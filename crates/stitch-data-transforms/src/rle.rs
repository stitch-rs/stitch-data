//! Run-Length Encoding (RLE) transformer for repetitive sequences.

use stitch_data_core::error::CbpError;

/// Stateless Run-Length Encoding transformer.
pub struct RleTransformer;

impl RleTransformer {
    /// Encodes a slice of `u32` into pairs of `(count: u16, value: u32)`.
    /// Format: `[NumPairs: u32]` + `[(Count: u16, Value: u32)]*`
    pub fn encode_u32(src: &[u32], dst: &mut [u8]) -> Result<usize, CbpError> {
        if src.is_empty() {
            if dst.len() < 4 {
                return Err(CbpError::UnexpectedEof {
                    required: 4,
                    available: dst.len(),
                });
            }
            dst[..4].copy_from_slice(&0u32.to_le_bytes());
            return Ok(4);
        }

        let mut offset = 4;
        let mut pairs_count = 0u32;

        let mut current_val = src[0];
        let mut current_run = 1u16;

        for &val in &src[1..] {
            if val == current_val && current_run < u16::MAX {
                current_run += 1;
            } else {
                if dst.len() < offset + 6 {
                    return Err(CbpError::UnexpectedEof {
                        required: offset + 6,
                        available: dst.len(),
                    });
                }
                dst[offset..offset + 2].copy_from_slice(&current_run.to_le_bytes());
                dst[offset + 2..offset + 6].copy_from_slice(&current_val.to_le_bytes());
                offset += 6;
                pairs_count += 1;

                current_val = val;
                current_run = 1;
            }
        }

        // Flush last pair
        if dst.len() < offset + 6 {
            return Err(CbpError::UnexpectedEof {
                required: offset + 6,
                available: dst.len(),
            });
        }
        dst[offset..offset + 2].copy_from_slice(&current_run.to_le_bytes());
        dst[offset + 2..offset + 6].copy_from_slice(&current_val.to_le_bytes());
        offset += 6;
        pairs_count += 1;

        dst[..4].copy_from_slice(&pairs_count.to_le_bytes());
        Ok(offset)
    }

    /// Decodes an RLE sequence into a destination slice.
    ///
    /// Returns `(elements_decoded, bytes_consumed)`.
    pub fn decode_u32_ext(src: &[u8], dst: &mut [u32]) -> Result<(usize, usize), CbpError> {
        if src.len() < 4 {
            return Err(CbpError::UnexpectedEof {
                required: 4,
                available: src.len(),
            });
        }

        let raw_pairs = u32::from_le_bytes(src[..4].try_into().unwrap());
        let pairs_count = usize::try_from(raw_pairs).map_err(|_| CbpError::PayloadDecodeError)?;
        let pairs_bytes = pairs_count
            .checked_mul(6)
            .ok_or(CbpError::PayloadDecodeError)?;
        let expected_bytes = 4usize
            .checked_add(pairs_bytes)
            .ok_or(CbpError::PayloadDecodeError)?;

        if src.len() < expected_bytes {
            return Err(CbpError::UnexpectedEof {
                required: expected_bytes,
                available: src.len(),
            });
        }

        let mut offset = 4;
        let mut dst_idx = 0;

        for _ in 0..pairs_count {
            let run = u16::from_le_bytes(src[offset..offset + 2].try_into().unwrap()) as usize;
            let val = u32::from_le_bytes(src[offset + 2..offset + 6].try_into().unwrap());
            offset += 6;

            for _ in 0..run {
                if dst_idx >= dst.len() {
                    return Err(CbpError::PayloadDecodeError);
                }
                dst[dst_idx] = val;
                dst_idx += 1;
            }
        }

        Ok((dst_idx, offset))
    }

    /// Decodes an RLE sequence into a destination slice. Returns elements decoded.
    #[inline]
    pub fn decode_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, CbpError> {
        Self::decode_u32_ext(src, dst).map(|(elements, _bytes)| elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_roundtrip() {
        let original = [5u32, 5, 5, 5, 10, 10, 42, 42, 42, 42, 42];
        let mut encoded = [0u8; 128];
        let bytes_written = RleTransformer::encode_u32(&original, &mut encoded).unwrap();

        // 3 pairs: (4, 5), (2, 10), (5, 42) = 4 + 3 * 6 = 22 bytes
        assert_eq!(bytes_written, 22);

        let mut decoded = [0u32; 11];
        let items_decoded =
            RleTransformer::decode_u32(&encoded[..bytes_written], &mut decoded).unwrap();
        assert_eq!(items_decoded, 11);
        assert_eq!(decoded, original);
    }
}
