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

/// Maximum possible serialized length of a CBP Frame Header (20 bytes).
/// 1B (Magic) + 1B (Version) + 1B (Profile/Flags) + 9B (Max PrefixVarint) + 8B (Schema Hash) = 20B.
/// 24 bytes ensures 8-byte alignment padding.
pub const MAX_HEADER_SIZE: usize = 24;

/// A zero-allocation, scatter-gather vectored representation of a serialized CBP frame.
///
/// Encodes the header into a tiny stack buffer and borrows the payload directly,
/// computing the CRC32 checksum across both slices without intermediate memory allocation
/// or contiguous buffer assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectoredFrame<'a> {
    header_buf: [u8; MAX_HEADER_SIZE],
    header_len: usize,
    payload: &'a [u8],
    crc_buf: [u8; 4],
}

impl<'a> VectoredFrame<'a> {
    /// Returns the serialized frame header slice.
    #[inline]
    pub fn header(&self) -> &[u8] {
        &self.header_buf[..self.header_len]
    }

    /// Returns the borrowed payload slice.
    #[inline]
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Returns the 4-byte little-endian CRC32 Castagnoli checksum footer bytes.
    #[inline]
    pub fn crc_bytes(&self) -> &[u8; 4] {
        &self.crc_buf
    }

    /// Returns the calculated CRC32 checksum as a `u32`.
    #[inline]
    pub fn crc(&self) -> u32 {
        u32::from_le_bytes(self.crc_buf)
    }

    /// Returns the total wire length of the frame in bytes (`header_len + payload.len() + 4`).
    #[inline]
    pub fn total_len(&self) -> usize {
        self.header_len + self.payload.len() + 4
    }

    /// Returns the 3 scatter-gather byte slices `[Header, Payload, CRC32]`.
    #[inline]
    pub fn slices(&self) -> [&[u8]; 3] {
        [self.header(), self.payload, &self.crc_buf]
    }

    /// Returns the slices as standard `std::io::IoSlice` references for direct `writev` dispatch.
    #[cfg(feature = "std")]
    #[inline]
    pub fn io_slices(&self) -> [std::io::IoSlice<'_>; 3] {
        [
            std::io::IoSlice::new(self.header()),
            std::io::IoSlice::new(self.payload),
            std::io::IoSlice::new(&self.crc_buf),
        ]
    }

    /// Copies the entire vectored frame contiguously into `dst`. Returns total bytes written.
    #[inline]
    pub fn copy_to_slice(&self, dst: &mut [u8]) -> Result<usize, CbpError> {
        let total = self.total_len();
        if dst.len() < total {
            return Err(CbpError::UnexpectedEof {
                required: total,
                available: dst.len(),
            });
        }

        let h_len = self.header_len;
        let p_len = self.payload.len();

        dst[..h_len].copy_from_slice(self.header());
        dst[h_len..h_len + p_len].copy_from_slice(self.payload);
        dst[h_len + p_len..total].copy_from_slice(&self.crc_buf);

        Ok(total)
    }
}

/// Encodes a CBP frame in scatter-gather vectored mode with zero payload copying and zero heap allocations.
///
/// Prepares a stack-allocated header and calculates the CRC32-C checksum across both the header
/// and the borrowed payload slice using `crc32_castagnoli_slices`.
pub fn pack_frame_vectored<'a>(
    header: &FrameHeader,
    payload: &'a [u8],
) -> Result<VectoredFrame<'a>, CbpError> {
    let mut header_buf = [0u8; MAX_HEADER_SIZE];
    let mut effective_header = header.clone();
    effective_header.payload_len = payload.len();
    let header_len = effective_header.write_into(&mut header_buf)?;

    let crc = crate::crc::crc32_castagnoli_slices(&[&header_buf[..header_len], payload]);

    Ok(VectoredFrame {
        header_buf,
        header_len,
        payload,
        crc_buf: crc.to_le_bytes(),
    })
}

/// A zero-copy borrowed view of an unpacked CBP frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameView<'a> {
    /// Parsed frame header.
    pub header: FrameHeader,
    /// Direct borrowed slice into the frame payload.
    pub payload: &'a [u8],
    /// Verified Castagnoli CRC32 checksum from the footer.
    pub crc: u32,
    /// Total wire bytes consumed by this frame (`header_len + payload_len + 4`).
    pub frame_len: usize,
}

/// A zero-allocation, cursor-based streaming reader that yields successive frames from a borrowed slice.
///
/// Efficiently processes multi-frame buffers (e.g. TCP buffers, IPC shared memory, ring buffers)
/// with zero heap allocations and zero memory copies.
#[derive(Debug, Clone)]
pub struct FrameReader<'a> {
    buffer: &'a [u8],
    cursor: usize,
}

impl<'a> FrameReader<'a> {
    /// Creates a new `FrameReader` over the given borrowed buffer.
    #[inline]
    pub const fn new(buffer: &'a [u8]) -> Self {
        Self { buffer, cursor: 0 }
    }

    /// Returns the slice of unconsumed bytes remaining in the buffer.
    #[inline]
    pub fn remaining(&self) -> &'a [u8] {
        &self.buffer[self.cursor..]
    }

    /// Returns the number of bytes consumed so far.
    #[inline]
    pub const fn consumed(&self) -> usize {
        self.cursor
    }

    /// Returns `true` if all bytes in the buffer have been consumed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cursor >= self.buffer.len()
    }

    /// Decodes and verifies the next CBP frame from the buffer, advancing the cursor.
    ///
    /// - Returns `Ok(Some(frame))` if a valid frame was unpacked and verified.
    /// - Returns `Ok(None)` if all bytes have been consumed (`remaining().is_empty()`).
    /// - Returns `Err(CbpError::UnexpectedEof { .. })` if the remaining buffer contains a partial frame.
    /// - Returns `Err(...)` if the magic byte, version, or CRC32 checksum is invalid.
    pub fn next_frame(&mut self) -> Result<Option<FrameView<'a>>, CbpError> {
        let remaining = self.remaining();
        if remaining.is_empty() {
            return Ok(None);
        }

        let (header, header_len) = FrameHeader::parse(remaining)?;
        let expected_total = header_len + header.payload_len + 4;

        if remaining.len() < expected_total {
            return Err(CbpError::UnexpectedEof {
                required: expected_total,
                available: remaining.len(),
            });
        }

        let payload = &remaining[header_len..header_len + header.payload_len];
        let expected_crc = u32::from_le_bytes(
            remaining[expected_total - 4..expected_total]
                .try_into()
                .unwrap(),
        );
        let actual_crc =
            crate::crc::crc32_castagnoli(&remaining[..header_len + header.payload_len]);

        if expected_crc != actual_crc {
            return Err(CbpError::ChecksumMismatch {
                expected: expected_crc,
                actual: actual_crc,
            });
        }

        self.cursor += expected_total;

        Ok(Some(FrameView {
            header,
            payload,
            crc: expected_crc,
            frame_len: expected_total,
        }))
    }
}

impl<'a> Iterator for FrameReader<'a> {
    type Item = Result<FrameView<'a>, CbpError>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self.next_frame() {
            Ok(Some(view)) => Some(Ok(view)),
            Ok(None) => None,
            Err(e) => {
                // Advance cursor to end on error to terminate iterator safely
                self.cursor = self.buffer.len();
                Some(Err(e))
            }
        }
    }
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
    fn test_vectored_frame_parity_with_pack_frame() {
        let payload = b"Vectored scatter-gather payload";
        let header = FrameHeader {
            profile: OptimizationProfile::Realtime,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: false,
                has_schema_hash: true,
            },
            payload_len: payload.len(),
            schema_hash: Some(0xA1B2C3D4E5F60718),
        };

        let mut contiguous = [0u8; 128];
        let packed_len = pack_frame(&header, payload, &mut contiguous).unwrap();

        let vectored = pack_frame_vectored(&header, payload).unwrap();
        assert_eq!(vectored.total_len(), packed_len);
        assert_eq!(vectored.header(), &contiguous[..vectored.header().len()]);
        assert_eq!(vectored.payload(), payload);
        assert_eq!(
            vectored.crc_bytes(),
            &contiguous[packed_len - 4..packed_len]
        );

        let mut copied = [0u8; 128];
        let copied_len = vectored.copy_to_slice(&mut copied).unwrap();
        assert_eq!(&copied[..copied_len], &contiguous[..packed_len]);
    }

    #[test]
    fn test_frame_reader_multi_frame_stream() {
        let payload1 = b"Frame number 1";
        let payload2 = b"Frame number 2: longer payload contents";
        let payload3 = b"Frame number 3: end of sequence";

        let hdr1 = FrameHeader {
            profile: OptimizationProfile::Direct,
            flags: FrameFlags::default(),
            payload_len: payload1.len(),
            schema_hash: None,
        };
        let hdr2 = FrameHeader {
            profile: OptimizationProfile::Realtime,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: false,
                has_schema_hash: false,
            },
            payload_len: payload2.len(),
            schema_hash: None,
        };
        let hdr3 = FrameHeader {
            profile: OptimizationProfile::MaxDensity,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: true,
                has_schema_hash: true,
            },
            payload_len: payload3.len(),
            schema_hash: Some(0x1234),
        };

        let mut buffer = [0u8; 512];
        let mut offset = 0;
        offset += pack_frame(&hdr1, payload1, &mut buffer[offset..]).unwrap();
        offset += pack_frame(&hdr2, payload2, &mut buffer[offset..]).unwrap();
        offset += pack_frame(&hdr3, payload3, &mut buffer[offset..]).unwrap();

        let total_stream = &buffer[..offset];

        let mut reader = FrameReader::new(total_stream);

        let f1 = reader.next_frame().unwrap().expect("frame 1 must exist");
        assert_eq!(f1.payload, payload1);
        assert_eq!(f1.header.profile, OptimizationProfile::Direct);

        let f2 = reader.next_frame().unwrap().expect("frame 2 must exist");
        assert_eq!(f2.payload, payload2);
        assert_eq!(f2.header.profile, OptimizationProfile::Realtime);

        let f3 = reader.next_frame().unwrap().expect("frame 3 must exist");
        assert_eq!(f3.payload, payload3);
        assert_eq!(f3.header.profile, OptimizationProfile::MaxDensity);

        assert!(
            reader.next_frame().unwrap().is_none(),
            "stream must be exhausted"
        );
        assert!(reader.is_empty());
        assert_eq!(reader.consumed(), offset);
    }

    #[test]
    fn test_frame_reader_partial_frame_unexpected_eof() {
        let payload = b"Partial frame testing payload";
        let header = FrameHeader {
            profile: OptimizationProfile::Balanced,
            flags: FrameFlags::default(),
            payload_len: payload.len(),
            schema_hash: None,
        };

        let mut buf = [0u8; 128];
        let total = pack_frame(&header, payload, &mut buf).unwrap();

        // Truncate by 5 bytes
        let truncated = &buf[..total - 5];
        let mut reader = FrameReader::new(truncated);
        let err = reader.next_frame().unwrap_err();

        match err {
            CbpError::UnexpectedEof {
                required,
                available,
            } => {
                assert_eq!(required, total);
                assert_eq!(available, total - 5);
            }
            other => panic!("expected UnexpectedEof, got {:?}", other),
        }
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
