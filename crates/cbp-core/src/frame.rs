//! CBP Frame framing structures according to Version 1 Axiom.

use crate::crc::crc32_castagnoli;
use crate::error::CbpError;
use crate::varint::{decode_prefix_varint, encode_prefix_varint};

/// Canonical Magic byte for Compact Binary Protocol (`0xCB`).
pub const CBP_MAGIC: u8 = 0xCB;

/// Version 1 identifier.
pub const CBP_VERSION_1: u8 = 0x01;

/// Layered optimization profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OptimizationProfile {
    /// -O0: Direct zero-copy without transformations.
    Direct = 0b00,
    /// -O1: Realtime sub-microsecond SIMD bit-packing and prefix varints.
    Realtime = 0b01,
    /// -O2: Balanced delta-encoding, PFOR, and RLE.
    Balanced = 0b10,
    /// -O3: Maximum density (columnar SoA, Elias-Fano, dictionary).
    MaxDensity = 0b11,
}

impl OptimizationProfile {
    /// Converts a 2-bit value into an `OptimizationProfile`.
    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Self::Direct,
            0b01 => Self::Realtime,
            0b10 => Self::Balanced,
            _ => Self::MaxDensity,
        }
    }
}

/// Bitwise frame attribute flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameFlags {
    /// Payload is compressed using profile transforms.
    pub is_compressed: bool,
    /// Payload is organized into columnar or chunked blocks.
    pub is_chunked: bool,
    /// Header includes a 64-bit schema fingerprint.
    pub has_schema_hash: bool,
}

impl FrameFlags {
    /// Converts flags into a 4-bit representation.
    #[inline]
    pub const fn to_bits(&self) -> u8 {
        let mut bits = 0u8;
        if self.is_compressed {
            bits |= 0x1;
        }
        if self.is_chunked {
            bits |= 0x2;
        }
        if self.has_schema_hash {
            bits |= 0x4;
        }
        bits
    }

    /// Parses flags from a 4-bit representation.
    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        Self {
            is_compressed: (bits & 0x1) != 0,
            is_chunked: (bits & 0x2) != 0,
            has_schema_hash: (bits & 0x4) != 0,
        }
    }
}

/// In-memory representation of a CBP Frame Header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// Selected optimization profile.
    pub profile: OptimizationProfile,
    /// Frame flags.
    pub flags: FrameFlags,
    /// Payload length in bytes.
    pub payload_len: usize,
    /// Optional schema fingerprint (xxHash64).
    pub schema_hash: Option<u64>,
}

impl FrameHeader {
    /// Serializes this frame header into `buf`. Returns bytes written.
    pub fn write_into(&self, buf: &mut [u8]) -> Result<usize, CbpError> {
        if buf.len() < 3 {
            return Err(CbpError::UnexpectedEof {
                required: 3,
                available: buf.len(),
            });
        }

        buf[0] = CBP_MAGIC;
        buf[1] = CBP_VERSION_1;

        let profile_bits = (self.profile as u8) & 0b11;
        let flags_bits = (self.flags.to_bits() & 0b1111) << 2;
        buf[2] = profile_bits | flags_bits;

        let mut offset = 3;

        // Encode payload length via PrefixVarint
        if buf.len() < offset + 9 {
            return Err(CbpError::UnexpectedEof {
                required: offset + 9,
                available: buf.len(),
            });
        }
        let varint_len = encode_prefix_varint(self.payload_len as u64, &mut buf[offset..]);
        offset += varint_len;

        // Encode optional schema fingerprint
        if let Some(hash) = self.schema_hash {
            if buf.len() < offset + 8 {
                return Err(CbpError::UnexpectedEof {
                    required: offset + 8,
                    available: buf.len(),
                });
            }
            buf[offset..offset + 8].copy_from_slice(&hash.to_le_bytes());
            offset += 8;
        }

        Ok(offset)
    }

    /// Parses a frame header from a byte slice. Returns `(FrameHeader, bytes_consumed)`.
    pub fn parse(buf: &[u8]) -> Result<(Self, usize), CbpError> {
        if buf.len() < 3 {
            return Err(CbpError::UnexpectedEof {
                required: 3,
                available: buf.len(),
            });
        }

        if buf[0] != CBP_MAGIC {
            return Err(CbpError::InvalidMagic(buf[0]));
        }
        if buf[1] != CBP_VERSION_1 {
            return Err(CbpError::UnsupportedVersion(buf[1]));
        }

        let byte2 = buf[2];
        let profile = OptimizationProfile::from_bits(byte2 & 0b11);
        let flags = FrameFlags::from_bits((byte2 >> 2) & 0b1111);

        let mut offset = 3;
        let (payload_len, varint_len) = decode_prefix_varint(&buf[offset..])?;
        offset += varint_len;

        let mut schema_hash = None;
        if flags.has_schema_hash {
            if buf.len() < offset + 8 {
                return Err(CbpError::UnexpectedEof {
                    required: offset + 8,
                    available: buf.len(),
                });
            }
            schema_hash = Some(u64::from_le_bytes(
                buf[offset..offset + 8].try_into().unwrap(),
            ));
            offset += 8;
        }

        Ok((
            Self {
                profile,
                flags,
                payload_len: payload_len as usize,
                schema_hash,
            },
            offset,
        ))
    }
}

/// Packs an entire CBP frame (Header + Payload + CRC32 Footer) into `out_buf`.
pub fn pack_frame(
    header: &FrameHeader,
    payload: &[u8],
    out_buf: &mut [u8],
) -> Result<usize, CbpError> {
    let header_len = header.write_into(out_buf)?;
    let total_len = header_len + payload.len() + 4;

    if out_buf.len() < total_len {
        return Err(CbpError::UnexpectedEof {
            required: total_len,
            available: out_buf.len(),
        });
    }

    out_buf[header_len..header_len + payload.len()].copy_from_slice(payload);

    let crc = crc32_castagnoli(&out_buf[..header_len + payload.len()]);
    out_buf[header_len + payload.len()..total_len].copy_from_slice(&crc.to_le_bytes());

    Ok(total_len)
}

/// Unpacks and validates a CBP frame from `buf`. Returns `(FrameHeader, payload_slice)`.
pub fn unpack_frame(buf: &[u8]) -> Result<(FrameHeader, &[u8]), CbpError> {
    let (header, header_len) = FrameHeader::parse(buf)?;
    let expected_total = header_len + header.payload_len + 4;

    if buf.len() < expected_total {
        return Err(CbpError::UnexpectedEof {
            required: expected_total,
            available: buf.len(),
        });
    }

    let payload = &buf[header_len..header_len + header.payload_len];
    let expected_crc =
        u32::from_le_bytes(buf[expected_total - 4..expected_total].try_into().unwrap());
    let actual_crc = crc32_castagnoli(&buf[..header_len + header.payload_len]);

    if expected_crc != actual_crc {
        return Err(CbpError::ChecksumMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    Ok((header, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_pack_unpack_roundtrip() {
        let payload = b"Hello CBP Engine from Rust!";
        let header = FrameHeader {
            profile: OptimizationProfile::Balanced,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: false,
                has_schema_hash: true,
            },
            payload_len: payload.len(),
            schema_hash: Some(0xDEADBEEFCAFE1234),
        };

        let mut storage = [0u8; 128];
        let total = pack_frame(&header, payload, &mut storage).unwrap();

        let (unpacked_hdr, unpacked_payload) = unpack_frame(&storage[..total]).unwrap();
        assert_eq!(unpacked_hdr, header);
        assert_eq!(unpacked_payload, payload);
    }

    #[test]
    fn test_corrupted_crc_detection() {
        let payload = b"Sensitive payload";
        let header = FrameHeader {
            profile: OptimizationProfile::Direct,
            flags: FrameFlags::default(),
            payload_len: payload.len(),
            schema_hash: None,
        };

        let mut storage = [0u8; 64];
        let total = pack_frame(&header, payload, &mut storage).unwrap();

        // Corrupt single payload byte
        storage[5] ^= 0xFF;

        let res = unpack_frame(&storage[..total]);
        assert!(matches!(res, Err(CbpError::ChecksumMismatch { .. })));
    }
}
