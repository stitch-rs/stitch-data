//! Packed bit-width integer compression.

use cbp_core::bitstream::{BitReader, BitWriter};
use cbp_core::error::CbpError;

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

        let mut writer = BitWriter::new(&mut dst[1..]);
        for &val in slice {
            writer.write_bits(val as u64, bit_width)?;
        }
        writer.align();

        Ok(1 + writer.bytes_written())
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

        let mut reader = BitReader::new(&src[1..]);
        for item in dst.iter_mut() {
            *item = reader.read_bits(bit_width)? as u32;
        }

        reader.align();
        Ok(1 + src.len() - reader.remaining_bytes().len())
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
        BitPacker::unpack_u32(&packed[..bytes_written], &mut unpacked).unwrap();
        assert_eq!(unpacked, original);
    }
}
