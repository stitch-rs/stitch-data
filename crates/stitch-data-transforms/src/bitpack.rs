//! Packed bit-width integer compression.
//!
//! Provides zero-allocation, SIMD-accelerated and SWAR fast-path bit packing
//! for 1..32 bit widths.

use stitch_data_core::error::CbpError;

/// Bit-packing compressor calculating optimal bit widths per block.
pub struct BitPacker;

impl BitPacker {
    /// Determines the minimum bit width required to represent the maximum value in `slice`.
    #[inline]
    pub fn required_bit_width(slice: &[u32]) -> u8 {
        let max = slice.iter().copied().max().unwrap_or(0);
        if max == 0 {
            1
        } else {
            32 - max.leading_zeros() as u8
        }
    }

    /// Packs a slice of `u32` into a byte buffer using minimal bit-width.
    /// Format: `[BitWidth: u8]` + `[PackedBits: BitWidth * len]`
    pub fn pack_u32(slice: &[u32], dst: &mut [u8]) -> Result<usize, CbpError> {
        if dst.is_empty() {
            return Err(CbpError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }

        let bit_width = Self::required_bit_width(slice);
        dst[0] = bit_width;

        let total_bits = slice
            .len()
            .checked_mul(bit_width as usize)
            .ok_or(CbpError::PayloadDecodeError)?;
        let packed_bytes = total_bits.div_ceil(8);
        let total_required = 1usize
            .checked_add(packed_bytes)
            .ok_or(CbpError::PayloadDecodeError)?;

        if dst.len() < total_required {
            return Err(CbpError::UnexpectedEof {
                required: total_required,
                available: dst.len(),
            });
        }

        if slice.is_empty() {
            return Ok(1);
        }

        let body = &mut dst[1..total_required];
        // Zero-fill the destination buffer to ensure clean unused trailing bits
        body.fill(0);

        match bit_width {
            1 => {
                for (i, &val) in slice.iter().enumerate() {
                    body[i / 8] |= ((val & 1) as u8) << (i % 8);
                }
            }
            4 => {
                let (chunks, remainder) = slice.as_chunks::<2>();
                let mut byte_idx = 0;
                for pair in chunks {
                    body[byte_idx] = (pair[0] as u8 & 0x0F) | ((pair[1] as u8 & 0x0F) << 4);
                    byte_idx += 1;
                }
                if let Some(&rem) = remainder.first() {
                    body[byte_idx] = rem as u8 & 0x0F;
                }
            }
            8 => {
                let (src_chunks, src_rem) = slice.as_chunks::<8>();
                let (dst_chunks, dst_rem) = body.as_chunks_mut::<8>();
                for (chunk_src, chunk_dst) in src_chunks.iter().zip(dst_chunks.iter_mut()) {
                    chunk_dst[0] = chunk_src[0] as u8;
                    chunk_dst[1] = chunk_src[1] as u8;
                    chunk_dst[2] = chunk_src[2] as u8;
                    chunk_dst[3] = chunk_src[3] as u8;
                    chunk_dst[4] = chunk_src[4] as u8;
                    chunk_dst[5] = chunk_src[5] as u8;
                    chunk_dst[6] = chunk_src[6] as u8;
                    chunk_dst[7] = chunk_src[7] as u8;
                }
                for (s, d) in src_rem.iter().zip(dst_rem.iter_mut()) {
                    *d = *s as u8;
                }
            }
            12 => {
                let (chunks, remainder) = slice.as_chunks::<2>();
                let mut b_idx = 0;
                for pair in chunks {
                    let v0 = pair[0];
                    let v1 = pair[1];
                    body[b_idx] = (v0 & 0xFF) as u8;
                    body[b_idx + 1] = ((v0 >> 8) as u8) | (((v1 & 0x0F) as u8) << 4);
                    body[b_idx + 2] = ((v1 >> 4) & 0xFF) as u8;
                    b_idx += 3;
                }
                if let Some(&v0) = remainder.first() {
                    body[b_idx] = (v0 & 0xFF) as u8;
                    body[b_idx + 1] = (v0 >> 8) as u8;
                }
            }
            16 => {
                for (i, &val) in slice.iter().enumerate() {
                    let le = (val as u16).to_le_bytes();
                    body[2 * i] = le[0];
                    body[2 * i + 1] = le[1];
                }
            }
            32 => {
                let byte_len = slice.len() * 4;
                // SAFETY: Casting &[u32] to &[u8] is sound as u32 is POD with no padding or uninitialized memory.
                let src_bytes =
                    unsafe { core::slice::from_raw_parts(slice.as_ptr() as *const u8, byte_len) };
                body[..byte_len].copy_from_slice(src_bytes);
            }
            _ => {
                // General SWAR bitstream writer
                let mut byte_pos = 0;
                let mut acc = 0u64;
                let mut bits_in_acc = 0u8;

                for &val in slice {
                    let mask = (1u64 << bit_width) - 1;
                    acc |= (val as u64 & mask) << bits_in_acc;
                    bits_in_acc += bit_width;

                    while bits_in_acc >= 8 {
                        body[byte_pos] = (acc & 0xFF) as u8;
                        acc >>= 8;
                        bits_in_acc -= 8;
                        byte_pos += 1;
                    }
                }

                if bits_in_acc > 0 {
                    body[byte_pos] = (acc & 0xFF) as u8;
                }
            }
        }

        Ok(total_required)
    }

    /// Unpacks a packed slice of `u32` from a byte buffer.
    pub fn unpack_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, CbpError> {
        if src.is_empty() {
            return Err(CbpError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }

        let bit_width = src[0];
        if bit_width == 0 || bit_width > 32 {
            return Err(CbpError::InvalidBitWidth(bit_width));
        }

        let total_bits = dst
            .len()
            .checked_mul(bit_width as usize)
            .ok_or(CbpError::PayloadDecodeError)?;
        let packed_bytes = total_bits.div_ceil(8);
        let total_required = 1usize
            .checked_add(packed_bytes)
            .ok_or(CbpError::PayloadDecodeError)?;

        if src.len() < total_required {
            return Err(CbpError::UnexpectedEof {
                required: total_required,
                available: src.len(),
            });
        }

        if dst.is_empty() {
            return Ok(1);
        }

        let body = &src[1..total_required];

        match bit_width {
            1 => {
                for (i, item) in dst.iter_mut().enumerate() {
                    let byte = body[i / 8];
                    *item = ((byte >> (i % 8)) & 1) as u32;
                }
            }
            4 => {
                let (chunks, remainder) = dst.as_chunks_mut::<2>();
                let mut byte_idx = 0;
                for pair in chunks {
                    let b = body[byte_idx];
                    pair[0] = (b & 0x0F) as u32;
                    pair[1] = ((b >> 4) & 0x0F) as u32;
                    byte_idx += 1;
                }
                if let Some(rem) = remainder.first_mut() {
                    let b = body[byte_idx];
                    *rem = (b & 0x0F) as u32;
                }
            }
            8 => {
                #[cfg(target_arch = "x86_64")]
                {
                    if crate::simd::has_avx2() && dst.len() >= 8 {
                        // SAFETY: We verified has_avx2() at runtime and lengths are bounded by total_required.
                        unsafe {
                            crate::simd::x86::unpack_8bit_avx2(body, dst);
                        }
                        return Ok(total_required);
                    }
                }
                let (src_chunks, src_rem) = body.as_chunks::<8>();
                let (dst_chunks, dst_rem) = dst.as_chunks_mut::<8>();
                for (chunk_src, chunk_dst) in src_chunks.iter().zip(dst_chunks.iter_mut()) {
                    chunk_dst[0] = chunk_src[0] as u32;
                    chunk_dst[1] = chunk_src[1] as u32;
                    chunk_dst[2] = chunk_src[2] as u32;
                    chunk_dst[3] = chunk_src[3] as u32;
                    chunk_dst[4] = chunk_src[4] as u32;
                    chunk_dst[5] = chunk_src[5] as u32;
                    chunk_dst[6] = chunk_src[6] as u32;
                    chunk_dst[7] = chunk_src[7] as u32;
                }
                for (s, d) in src_rem.iter().zip(dst_rem.iter_mut()) {
                    *d = *s as u32;
                }
            }
            12 => {
                let (chunks, remainder) = dst.as_chunks_mut::<2>();
                let mut b_idx = 0;
                for pair in chunks {
                    let b0 = body[b_idx] as u32;
                    let b1 = body[b_idx + 1] as u32;
                    let b2 = body[b_idx + 2] as u32;
                    pair[0] = b0 | ((b1 & 0x0F) << 8);
                    pair[1] = (b1 >> 4) | (b2 << 4);
                    b_idx += 3;
                }
                if let Some(rem) = remainder.first_mut() {
                    let b0 = body[b_idx] as u32;
                    let b1 = body[b_idx + 1] as u32;
                    *rem = b0 | ((b1 & 0x0F) << 8);
                }
            }
            16 => {
                #[cfg(target_arch = "x86_64")]
                {
                    if crate::simd::has_avx2() && dst.len() >= 8 {
                        // SAFETY: We verified has_avx2() at runtime and lengths are bounded by total_required.
                        unsafe {
                            crate::simd::x86::unpack_16bit_avx2(body, dst);
                        }
                        return Ok(total_required);
                    }
                }
                for (i, item) in dst.iter_mut().enumerate() {
                    let b0 = body[2 * i] as u32;
                    let b1 = body[2 * i + 1] as u32;
                    *item = b0 | (b1 << 8);
                }
            }
            32 => {
                let byte_len = dst.len() * 4;
                // SAFETY: dst is &mut [u32], which is 4-byte aligned and writable for byte_len bytes.
                let dst_bytes = unsafe {
                    core::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u8, byte_len)
                };
                dst_bytes.copy_from_slice(&body[..byte_len]);
            }
            _ => {
                // General SWAR bitstream reader
                let mut byte_pos = 0;
                let mut acc = 0u64;
                let mut bits_in_acc = 0u8;
                let mask = (1u64 << bit_width) - 1;

                for item in dst.iter_mut() {
                    while bits_in_acc < bit_width {
                        acc |= (body[byte_pos] as u64) << bits_in_acc;
                        bits_in_acc += 8;
                        byte_pos += 1;
                    }

                    *item = (acc & mask) as u32;
                    acc >>= bit_width;
                    bits_in_acc -= bit_width;
                }
            }
        }

        Ok(total_required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitpack_roundtrip() {
        let original = [0u32, 1, 2, 5, 7, 3, 6, 7]; // max 7 -> 3 bits
        assert_eq!(BitPacker::required_bit_width(&original), 3);

        let mut packed = [0u8; 32];
        let bytes_written = BitPacker::pack_u32(&original, &mut packed).unwrap();

        // 8 items * 3 bits = 24 bits = 3 bytes + 1 byte header = 4 bytes total
        assert_eq!(bytes_written, 4);

        let mut unpacked = [0u32; 8];
        let bytes_read = BitPacker::unpack_u32(&packed[..bytes_written], &mut unpacked).unwrap();
        assert_eq!(bytes_read, bytes_written);
        assert_eq!(unpacked, original);
    }

    #[test]
    fn test_bitpack_exact_bytes_consumed_various_widths() {
        for bit_width in 1..=32 {
            let max_val = if bit_width == 32 {
                u32::MAX
            } else {
                (1u32 << bit_width) - 1
            };
            let data: [u32; 17] = [max_val; 17];

            let mut packed = [0u8; 128];
            let written = BitPacker::pack_u32(&data, &mut packed).unwrap();

            let mut unpacked = [0u32; 17];
            let read = BitPacker::unpack_u32(&packed[..written], &mut unpacked).unwrap();

            assert_eq!(read, written, "width {}", bit_width);
            assert_eq!(unpacked, data, "width {}", bit_width);
        }
    }

    #[test]
    fn test_bitpack_all_widths_pattern_roundtrip() {
        for bit_width in 1..=32 {
            let max_val = if bit_width == 32 {
                u32::MAX
            } else {
                (1u32 << bit_width) - 1
            };

            // Test various lengths to test exact chunks and odd remainders
            for len in [1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64, 127, 128] {
                let mut original = [0u32; 128];
                for (i, val) in original[..len].iter_mut().enumerate() {
                    *val = ((i as u64 * 37 + 13) % (max_val as u64 + 1)) as u32;
                }

                let mut packed = [0u8; 1024];
                let written = BitPacker::pack_u32(&original[..len], &mut packed).unwrap();

                let mut unpacked = [0u32; 128];
                let read = BitPacker::unpack_u32(&packed[..written], &mut unpacked[..len]).unwrap();

                assert_eq!(read, written, "bit_width {} len {}", bit_width, len);
                assert_eq!(
                    &unpacked[..len],
                    &original[..len],
                    "bit_width {} len {}",
                    bit_width,
                    len
                );
            }
        }
    }
}
