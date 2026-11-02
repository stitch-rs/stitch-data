//! Lossless floating-point compression using the Gorilla (Facebook TSDB) XOR algorithm.
//!
//! Provides bit-exact compression for `f32` and `f64` sequential streams (e.g. telemetry,
//! velocities, positions, and coordinates) without precision loss.

use stitch_data_core::bitstream::{BitReader, BitWriter};
use stitch_data_core::error::StitchError;

/// Stateless compressor for floating-point streams using Gorilla XOR delta encoding.
pub struct GorillaCompressor;

impl GorillaCompressor {
    /// Compresses a slice of `f32` values into `dst`.
    ///
    /// Bit-exact: preserves all IEEE-754 bit representations including sign, mantissa,
    /// exponent, subnormals, NaNs, and infinities.
    pub fn compress_f32(src: &[f32], dst: &mut [u8]) -> Result<usize, StitchError> {
        if src.is_empty() {
            return Ok(0);
        }
        if dst.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: dst.len(),
            });
        }

        // Write first float verbatim (32 bits)
        let first_bits = src[0].to_bits();
        dst[0..4].copy_from_slice(&first_bits.to_le_bytes());

        if src.len() == 1 {
            return Ok(4);
        }

        let mut writer = BitWriter::new(&mut dst[4..]);
        let mut prev_bits = first_bits;
        let mut prev_leading = u8::MAX;
        let mut prev_trailing = u8::MAX;

        for &val in &src[1..] {
            let bits = val.to_bits();
            let xor = bits ^ prev_bits;

            if xor == 0 {
                // Value is identical to previous: single '0' control bit
                writer.write_bits(0, 1)?;
            } else {
                // Value differs: write control bit '1'
                writer.write_bits(1, 1)?;

                let leading = (xor.leading_zeros() as u8).min(31);
                let trailing = (xor.trailing_zeros() as u8).min(31);

                if prev_leading != u8::MAX && leading >= prev_leading && trailing >= prev_trailing {
                    // Control bit '0': reuse previous leading/trailing bounds
                    writer.write_bits(0, 1)?;
                    let meaningful_len = 32 - prev_leading - prev_trailing;
                    let meaningful_bits = (xor >> prev_trailing) as u64;
                    writer.write_bits(meaningful_bits, meaningful_len)?;
                } else {
                    // Control bit '1': new leading and trailing bounds
                    writer.write_bits(1, 1)?;
                    // Write 5 bits for leading zeros (0..31)
                    writer.write_bits(leading as u64, 5)?;
                    // Write 6 bits for meaningful length (1..32)
                    let meaningful_len = 32 - leading - trailing;
                    writer.write_bits(meaningful_len as u64, 6)?;
                    // Write meaningful bits
                    let meaningful_bits = (xor >> trailing) as u64;
                    writer.write_bits(meaningful_bits, meaningful_len)?;

                    prev_leading = leading;
                    prev_trailing = trailing;
                }
            }

            prev_bits = bits;
        }

        writer.align();
        Ok(4 + writer.bytes_written())
    }

    /// Decompresses an `f32` stream into `dst`.
    pub fn decompress_f32(src: &[u8], dst: &mut [f32]) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: src.len(),
            });
        }

        let first_bits = u32::from_le_bytes(src[0..4].try_into().unwrap());
        dst[0] = f32::from_bits(first_bits);

        if dst.len() == 1 {
            return Ok(4);
        }

        let mut reader = BitReader::new(&src[4..]);
        let mut prev_bits = first_bits;
        let mut prev_leading = 0u8;
        let mut prev_len = 0u8;

        for item in &mut dst[1..] {
            let is_diff = reader.read_bits(1)?;
            if is_diff == 0 {
                // Identical to previous
                *item = f32::from_bits(prev_bits);
            } else {
                let use_new_bounds = reader.read_bits(1)?;
                let (leading, len) = if use_new_bounds == 0 {
                    (prev_leading, prev_len)
                } else {
                    let lead = reader.read_bits(5)? as u8;
                    let l = reader.read_bits(6)? as u8;
                    prev_leading = lead;
                    prev_len = l;
                    (lead, l)
                };

                if (leading as usize) + (len as usize) > 32 || len == 0 {
                    return Err(StitchError::PayloadDecodeError);
                }

                let meaningful_bits = reader.read_bits(len)? as u32;
                let trailing = 32 - leading - len;
                let xor = meaningful_bits << trailing;
                let bits = prev_bits ^ xor;
                *item = f32::from_bits(bits);
                prev_bits = bits;
            }
        }

        reader.align();
        Ok(4 + reader.bytes_read())
    }

    /// Compresses a slice of `f64` values into `dst`.
    pub fn compress_f64(src: &[f64], dst: &mut [u8]) -> Result<usize, StitchError> {
        if src.is_empty() {
            return Ok(0);
        }
        if dst.len() < 8 {
            return Err(StitchError::UnexpectedEof {
                required: 8,
                available: dst.len(),
            });
        }

        let first_bits = src[0].to_bits();
        dst[0..8].copy_from_slice(&first_bits.to_le_bytes());

        if src.len() == 1 {
            return Ok(8);
        }

        let mut writer = BitWriter::new(&mut dst[8..]);
        let mut prev_bits = first_bits;
        let mut prev_leading = u8::MAX;
        let mut prev_trailing = u8::MAX;

        for &val in &src[1..] {
            let bits = val.to_bits();
            let xor = bits ^ prev_bits;

            if xor == 0 {
                writer.write_bits(0, 1)?;
            } else {
                writer.write_bits(1, 1)?;

                let leading = (xor.leading_zeros() as u8).min(63);
                let trailing = (xor.trailing_zeros() as u8).min(63);

                if prev_leading != u8::MAX && leading >= prev_leading && trailing >= prev_trailing {
                    writer.write_bits(0, 1)?;
                    let meaningful_len = 64 - prev_leading - prev_trailing;
                    let meaningful_bits = xor >> prev_trailing;
                    writer.write_bits(meaningful_bits, meaningful_len)?;
                } else {
                    writer.write_bits(1, 1)?;
                    // Write 6 bits for leading zeros (0..63)
                    writer.write_bits(leading as u64, 6)?;
                    // Write 7 bits for meaningful length (1..64)
                    let meaningful_len = 64 - leading - trailing;
                    writer.write_bits(meaningful_len as u64, 7)?;
                    let meaningful_bits = xor >> trailing;
                    writer.write_bits(meaningful_bits, meaningful_len)?;

                    prev_leading = leading;
                    prev_trailing = trailing;
                }
            }

            prev_bits = bits;
        }

        writer.align();
        Ok(8 + writer.bytes_written())
    }

    /// Decompresses an `f64` stream into `dst`.
    pub fn decompress_f64(src: &[u8], dst: &mut [f64]) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.len() < 8 {
            return Err(StitchError::UnexpectedEof {
                required: 8,
                available: src.len(),
            });
        }

        let first_bits = u64::from_le_bytes(src[0..8].try_into().unwrap());
        dst[0] = f64::from_bits(first_bits);

        if dst.len() == 1 {
            return Ok(8);
        }

        let mut reader = BitReader::new(&src[8..]);
        let mut prev_bits = first_bits;
        let mut prev_leading = 0u8;
        let mut prev_len = 0u8;

        for item in &mut dst[1..] {
            let is_diff = reader.read_bits(1)?;
            if is_diff == 0 {
                *item = f64::from_bits(prev_bits);
            } else {
                let use_new_bounds = reader.read_bits(1)?;
                let (leading, len) = if use_new_bounds == 0 {
                    (prev_leading, prev_len)
                } else {
                    let lead = reader.read_bits(6)? as u8;
                    let l = reader.read_bits(7)? as u8;
                    prev_leading = lead;
                    prev_len = l;
                    (lead, l)
                };

                if (leading as usize) + (len as usize) > 64 || len == 0 {
                    return Err(StitchError::PayloadDecodeError);
                }

                let meaningful_bits = reader.read_bits(len)?;
                let trailing = 64 - leading - len;
                let xor = meaningful_bits << trailing;
                let bits = prev_bits ^ xor;
                *item = f64::from_bits(bits);
                prev_bits = bits;
            }
        }

        reader.align();
        Ok(8 + reader.bytes_read())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gorilla_f32_roundtrip_telemetry() {
        // Simulating smooth physics coordinates (x, y, z deltas)
        let original: [f32; 16] = [
            100.0, 100.1, 100.2, 100.25, 100.25, 100.25, 100.5, 101.0, 101.0, 101.5, 102.0, 102.25,
            102.5, 103.0, 103.5, 104.0,
        ];

        let mut buffer = [0u8; 256];
        let written = GorillaCompressor::compress_f32(&original, &mut buffer).unwrap();

        // 16 floats raw = 64 bytes. Gorilla should compress repetitive/smooth floats significantly.
        assert!(
            written < 64,
            "Compressed size {} should be less than raw 64 bytes",
            written
        );

        let mut restored = [0.0f32; 16];
        let read = GorillaCompressor::decompress_f32(&buffer[..written], &mut restored).unwrap();

        assert_eq!(read, written);
        for i in 0..16 {
            assert_eq!(restored[i].to_bits(), original[i].to_bits());
        }
    }

    #[test]
    fn test_gorilla_f64_roundtrip_telemetry() {
        let original: [f64; 8] = [
            59.934280, 59.934281, 59.934285, 59.934285, 30.335098, 30.335100, 30.335105, 30.335110,
        ];

        let mut buffer = [0u8; 256];
        let written = GorillaCompressor::compress_f64(&original, &mut buffer).unwrap();
        assert!(written < 64);

        let mut restored = [0.0f64; 8];
        let read = GorillaCompressor::decompress_f64(&buffer[..written], &mut restored).unwrap();

        assert_eq!(read, written);
        for i in 0..8 {
            assert_eq!(restored[i].to_bits(), original[i].to_bits());
        }
    }

    #[test]
    fn test_gorilla_f32_special_values() {
        let original: [f32; 5] = [0.0, -0.0, f32::INFINITY, f32::NEG_INFINITY, 1.2345e-20];
        let mut buffer = [0u8; 64];
        let written = GorillaCompressor::compress_f32(&original, &mut buffer).unwrap();

        let mut restored = [0.0f32; 5];
        let read = GorillaCompressor::decompress_f32(&buffer[..written], &mut restored).unwrap();

        assert_eq!(read, written);
        for i in 0..5 {
            assert_eq!(restored[i].to_bits(), original[i].to_bits());
        }
    }
}
