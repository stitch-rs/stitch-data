//! Zero-allocation dictionary encoding for categorical and repeated values.
//!
//! Suitable for `#![no_std]` high-throughput telemetry, symbol IDs, strings, and enums.
//! Compacts repetitive sets of values into a bounded dictionary and a bit-packed
//! or compact index sequence.

use stitch_data_core::error::CbpError;

/// Stateless Dictionary encoder and decoder for 32-bit categorical values.
pub struct DictTransformer;

impl DictTransformer {
    /// Maximum supported dictionary entries for zero-allocation scratchpad operations.
    pub const MAX_ENTRIES: usize = 256;

    /// Encodes a slice of `u32` into a dictionary-compressed byte buffer.
    ///
    /// # Layout
    /// - `[DictSize: u16]` (number of unique dictionary entries, $K \le 256$)
    /// - `[DictEntries: u32 * DictSize]` (little-endian dictionary symbols)
    /// - `[Indices: BitPacked or U8/U16]`
    ///   - If `DictSize <= 1`: 0 bits per index (implicit)
    ///   - If `DictSize <= 256`: 1 byte per index (or bitpacked if width <= 8)
    ///
    /// Returns the number of bytes written to `dst`.
    pub fn encode_u32(src: &[u32], dst: &mut [u8]) -> Result<usize, CbpError> {
        if src.is_empty() {
            if dst.len() < 2 {
                return Err(CbpError::UnexpectedEof {
                    required: 2,
                    available: dst.len(),
                });
            }
            dst[..2].copy_from_slice(&0u16.to_le_bytes());
            return Ok(2);
        }

        // 1. Build dictionary table in stack scratchpad (no-std, zero-alloc)
        let mut dict = [0u32; Self::MAX_ENTRIES];
        let mut dict_len = 0usize;

        for &val in src {
            // Linear search over small bounded dictionary (up to MAX_ENTRIES)
            let mut found = false;
            for entry in &dict[..dict_len] {
                if *entry == val {
                    found = true;
                    break;
                }
            }
            if !found {
                if dict_len >= Self::MAX_ENTRIES {
                    // Dictionary overflow - fallback or report error
                    return Err(CbpError::PayloadDecodeError);
                }
                dict[dict_len] = val;
                dict_len += 1;
            }
        }

        // Header: dict_len as u16
        let dict_header_len = 2usize;
        let dict_bytes = dict_len
            .checked_mul(4)
            .ok_or(CbpError::PayloadDecodeError)?;
        let header_total = dict_header_len
            .checked_add(dict_bytes)
            .ok_or(CbpError::PayloadDecodeError)?;

        if dst.len() < header_total {
            return Err(CbpError::UnexpectedEof {
                required: header_total,
                available: dst.len(),
            });
        }

        dst[..2].copy_from_slice(&(dict_len as u16).to_le_bytes());
        let mut offset = 2;
        for &entry in &dict[..dict_len] {
            dst[offset..offset + 4].copy_from_slice(&entry.to_le_bytes());
            offset += 4;
        }

        // 2. Encode indices
        if dict_len <= 1 {
            // All values are identical; no index storage needed
            Ok(offset)
        } else if dict_len <= 256 {
            // Store indices as u8
            let indices_len = src.len();
            let total_required = offset
                .checked_add(indices_len)
                .ok_or(CbpError::PayloadDecodeError)?;
            if dst.len() < total_required {
                return Err(CbpError::UnexpectedEof {
                    required: total_required,
                    available: dst.len(),
                });
            }

            for &val in src {
                let idx = dict[..dict_len].iter().position(|&e| e == val).unwrap_or(0) as u8;
                dst[offset] = idx;
                offset += 1;
            }
            Ok(offset)
        } else {
            Err(CbpError::PayloadDecodeError)
        }
    }

    /// Decodes a dictionary-compressed stream into `dst`.
    ///
    /// Returns `(elements_decoded, bytes_consumed)`.
    pub fn decode_u32(src: &[u8], dst: &mut [u32]) -> Result<(usize, usize), CbpError> {
        if src.len() < 2 {
            return Err(CbpError::UnexpectedEof {
                required: 2,
                available: src.len(),
            });
        }

        let dict_len = u16::from_le_bytes(src[..2].try_into().unwrap()) as usize;
        let dict_bytes = dict_len
            .checked_mul(4)
            .ok_or(CbpError::PayloadDecodeError)?;
        let header_total = 2usize
            .checked_add(dict_bytes)
            .ok_or(CbpError::PayloadDecodeError)?;

        if src.len() < header_total {
            return Err(CbpError::UnexpectedEof {
                required: header_total,
                available: src.len(),
            });
        }

        if dict_len > Self::MAX_ENTRIES {
            return Err(CbpError::PayloadDecodeError);
        }

        let mut dict = [0u32; Self::MAX_ENTRIES];
        let mut offset = 2;
        for entry in dict[..dict_len].iter_mut() {
            *entry = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap());
            offset += 4;
        }

        if dict_len == 0 {
            return Ok((0, offset));
        }

        if dict_len == 1 {
            // All output elements take dict[0]
            let fill_val = dict[0];
            for item in dst.iter_mut() {
                *item = fill_val;
            }
            return Ok((dst.len(), offset));
        }

        // Indices as u8
        let available_indices = src.len() - offset;
        let to_decode = dst.len().min(available_indices);

        for (i, item) in dst[..to_decode].iter_mut().enumerate() {
            let idx = src[offset + i] as usize;
            if idx >= dict_len {
                return Err(CbpError::PayloadDecodeError);
            }
            *item = dict[idx];
        }

        offset += to_decode;
        Ok((to_decode, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dict_u32_roundtrip_basic() {
        let original = [100u32, 200, 100, 300, 200, 100, 100, 300];
        let mut buf = [0u8; 128];
        let written = DictTransformer::encode_u32(&original, &mut buf).unwrap();

        let mut decoded = [0u32; 8];
        let (num_decoded, consumed) =
            DictTransformer::decode_u32(&buf[..written], &mut decoded).unwrap();

        assert_eq!(num_decoded, 8);
        assert_eq!(consumed, written);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_dict_u32_single_symbol() {
        let original = [42u32; 16];
        let mut buf = [0u8; 64];
        let written = DictTransformer::encode_u32(&original, &mut buf).unwrap();

        // 2 bytes dict_len + 4 bytes entry = 6 bytes total!
        assert_eq!(written, 6);

        let mut decoded = [0u32; 16];
        let (num_decoded, consumed) =
            DictTransformer::decode_u32(&buf[..written], &mut decoded).unwrap();
        assert_eq!(num_decoded, 16);
        assert_eq!(consumed, 6);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_dict_u32_empty() {
        let original: [u32; 0] = [];
        let mut buf = [0u8; 16];
        let written = DictTransformer::encode_u32(&original, &mut buf).unwrap();
        assert_eq!(written, 2);

        let mut decoded: [u32; 0] = [];
        let (num_decoded, consumed) =
            DictTransformer::decode_u32(&buf[..written], &mut decoded).unwrap();
        assert_eq!(num_decoded, 0);
        assert_eq!(consumed, 2);
    }
}
