//! Bit-level streaming reader and writer primitives.
//!
//! Provides zero-allocation, sub-byte bit packing into continuous memory buffers.

use crate::error::CbpError;

/// Bit-level stream writer operating over an existing mutable byte buffer.
pub struct BitWriter<'a> {
    buf: &'a mut [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..8 bits within the current byte
}

impl<'a> BitWriter<'a> {
    /// Creates a new bit writer wrapping a mutable byte slice.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        if !buf.is_empty() {
            buf[0] = 0;
        }
        Self {
            buf,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Writes `count` bits (1..=64) from `value` into the stream.
    #[inline]
    pub fn write_bits(&mut self, mut value: u64, mut count: u8) -> Result<(), CbpError> {
        if count == 0 {
            return Ok(());
        }
        if count > 64 {
            return Err(CbpError::InvalidBitWidth(count));
        }

        while count > 0 {
            if self.byte_pos >= self.buf.len() {
                return Err(CbpError::UnexpectedEof {
                    required: self.byte_pos + 1,
                    available: self.buf.len(),
                });
            }

            let bits_left_in_byte = 8 - self.bit_pos;
            let bits_to_write = count.min(bits_left_in_byte);

            let mask = (1u64 << bits_to_write) - 1;
            let chunk = ((value & mask) as u8) << self.bit_pos;

            self.buf[self.byte_pos] |= chunk;

            self.bit_pos += bits_to_write;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
                if self.byte_pos < self.buf.len() {
                    self.buf[self.byte_pos] = 0;
                }
            }

            value >>= bits_to_write;
            count -= bits_to_write;
        }

        Ok(())
    }

    /// Flushes any pending unaligned bits by advancing to the next byte boundary.
    #[inline]
    pub fn align(&mut self) {
        if self.bit_pos > 0 {
            self.byte_pos += 1;
            self.bit_pos = 0;
            if self.byte_pos < self.buf.len() {
                self.buf[self.byte_pos] = 0;
            }
        }
    }

    /// Returns the number of fully or partially consumed bytes written so far.
    #[inline]
    pub fn bytes_written(&self) -> usize {
        if self.bit_pos > 0 {
            self.byte_pos + 1
        } else {
            self.byte_pos
        }
    }
}

/// Bit-level stream reader operating over a read-only byte slice.
pub struct BitReader<'a> {
    buf: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    /// Creates a new bit reader over a continuous byte slice.
    #[inline]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Reads `count` bits (1..=64) from the stream into a `u64`.
    #[inline]
    pub fn read_bits(&mut self, mut count: u8) -> Result<u64, CbpError> {
        if count == 0 {
            return Ok(0);
        }
        if count > 64 {
            return Err(CbpError::InvalidBitWidth(count));
        }

        let mut result = 0u64;
        let mut bits_read = 0u8;

        while count > 0 {
            if self.byte_pos >= self.buf.len() {
                return Err(CbpError::UnexpectedEof {
                    required: self.byte_pos + 1,
                    available: self.buf.len(),
                });
            }

            let bits_left_in_byte = 8 - self.bit_pos;
            let bits_to_read = count.min(bits_left_in_byte);

            let mask = ((1u16 << bits_to_read) - 1) as u8;
            let chunk = (self.buf[self.byte_pos] >> self.bit_pos) & mask;

            result |= (chunk as u64) << bits_read;

            self.bit_pos += bits_to_read;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }

            bits_read += bits_to_read;
            count -= bits_to_read;
        }

        Ok(result)
    }

    /// Aligns the bit reader to the next byte boundary.
    #[inline]
    pub fn align(&mut self) {
        if self.bit_pos > 0 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }
    }

    /// Returns the remaining bytes from the current byte-aligned position.
    #[inline]
    pub fn remaining_bytes(&self) -> &'a [u8] {
        let offset = if self.bit_pos > 0 {
            (self.byte_pos + 1).min(self.buf.len())
        } else {
            self.byte_pos.min(self.buf.len())
        };
        &self.buf[offset..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_writer_reader_roundtrip() {
        let mut storage = [0u8; 32];
        let mut writer = BitWriter::new(&mut storage);

        writer.write_bits(0b101, 3).unwrap(); // 3 bits
        writer.write_bits(0b1111, 4).unwrap(); // 4 bits
        writer.write_bits(1, 1).unwrap(); // 1 bit (fills byte 0)
        writer.write_bits(42, 8).unwrap(); // 8 bits (byte 1)
        writer.write_bits(0xABCD, 16).unwrap(); // 16 bits (bytes 2, 3)

        let written = writer.bytes_written();
        assert_eq!(written, 4);

        let mut reader = BitReader::new(&storage[..written]);
        assert_eq!(reader.read_bits(3).unwrap(), 0b101);
        assert_eq!(reader.read_bits(4).unwrap(), 0b1111);
        assert_eq!(reader.read_bits(1).unwrap(), 1);
        assert_eq!(reader.read_bits(8).unwrap(), 42);
        assert_eq!(reader.read_bits(16).unwrap(), 0xABCD);
    }
}
