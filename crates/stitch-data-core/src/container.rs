//! Binary Container structures, directory index, and chunk descriptors for Stitch Data files.
//!
//! Provides self-describing `#![no_std]` headers, footers, and index entries for
//! chunked columnar containers with zero heap allocations on reading.

use crate::crc::crc32_castagnoli;
use crate::error::StitchError;

/// Stitch Data Container Magic bytes: ASCII `STDC` (0x43445453 little-endian).
pub const CONTAINER_MAGIC: [u8; 4] = *b"STDC";

/// Container generation format version (1.0).
pub const CONTAINER_VERSION_1: u16 = 1;

/// Flag indicating that chunk directory uses 16-byte `CompactChunkDescriptor` instead of legacy 24-byte `ChunkDescriptor`.
pub const FLAG_COMPACT_DIRECTORY: u16 = 0x0002;

/// Fixed-size container file header (16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ContainerHeader {
    /// Magic identifier (`STDC`).
    pub magic: [u8; 4],
    /// Container format generation version.
    pub version: u16,
    /// Container feature flags.
    pub flags: u16,
    /// Schema hash identifying payload record layout.
    pub schema_hash: u64,
}

impl ContainerHeader {
    pub const SIZE: usize = 16;

    /// Creates a new `ContainerHeader`.
    pub const fn new(schema_hash: u64, flags: u16) -> Self {
        Self {
            magic: CONTAINER_MAGIC,
            version: CONTAINER_VERSION_1,
            flags,
            schema_hash,
        }
    }

    /// Serializes the header into a byte slice.
    pub fn write_to(&self, dst: &mut [u8]) -> Result<(), StitchError> {
        if dst.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: dst.len(),
            });
        }
        dst[0..4].copy_from_slice(&self.magic);
        dst[4..6].copy_from_slice(&self.version.to_le_bytes());
        dst[6..8].copy_from_slice(&self.flags.to_le_bytes());
        dst[8..16].copy_from_slice(&self.schema_hash.to_le_bytes());
        Ok(())
    }

    /// Parses the header from a byte slice.
    pub fn parse(src: &[u8]) -> Result<Self, StitchError> {
        if src.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: src.len(),
            });
        }
        if src[0..4] != CONTAINER_MAGIC {
            return Err(StitchError::InvalidMagic(src[0]));
        }
        let version = u16::from_le_bytes(src[4..6].try_into().unwrap());
        if version != CONTAINER_VERSION_1 {
            return Err(StitchError::UnsupportedVersion(version as u8));
        }
        let flags = u16::from_le_bytes(src[6..8].try_into().unwrap());
        let schema_hash = u64::from_le_bytes(src[8..16].try_into().unwrap());

        Ok(Self {
            magic: CONTAINER_MAGIC,
            version,
            flags,
            schema_hash,
        })
    }
}

/// Self-describing chunk entry in the container index directory.
///
/// Enables zero-allocation $O(\log_2 M)$ binary search over container chunks
/// based on the `first_key` (e.g. initial `ip_from` or `timestamp`) of each chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ChunkDescriptor {
    /// Initial primary ordering key in this chunk (e.g. `ip_from`, `timestamp`).
    pub first_key: u64,
    /// Absolute byte offset of chunk payload in the container file.
    pub file_offset: u64,
    /// Compressed byte length of the chunk frame.
    pub compressed_len: u32,
    /// Total logical row count in this chunk.
    pub row_count: u32,
}

impl ChunkDescriptor {
    pub const SIZE: usize = 24;

    /// Serializes chunk descriptor to bytes.
    pub fn write_to(&self, dst: &mut [u8]) -> Result<(), StitchError> {
        if dst.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: dst.len(),
            });
        }
        dst[0..8].copy_from_slice(&self.first_key.to_le_bytes());
        dst[8..16].copy_from_slice(&self.file_offset.to_le_bytes());
        dst[16..20].copy_from_slice(&self.compressed_len.to_le_bytes());
        dst[20..24].copy_from_slice(&self.row_count.to_le_bytes());
        Ok(())
    }

    /// Parses chunk descriptor from bytes.
    pub fn parse(src: &[u8]) -> Result<Self, StitchError> {
        if src.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: src.len(),
            });
        }
        Ok(Self {
            first_key: u64::from_le_bytes(src[0..8].try_into().unwrap()),
            file_offset: u64::from_le_bytes(src[8..16].try_into().unwrap()),
            compressed_len: u32::from_le_bytes(src[16..20].try_into().unwrap()),
            row_count: u32::from_le_bytes(src[20..24].try_into().unwrap()),
        })
    }
}

/// Ultra-dense 16-byte chunk descriptor using `odd-int::U24` and relative scaled offset.
///
/// Conforms to the "Scrooge" Systems Mindset:
/// - Eliminates cache line straddling: $16 \times 4 = 64$ bytes (fits **exactly 4 descriptors per 64-byte L1D cache line**).
/// - 33.3% smaller directory index tables compared to legacy 24-byte `ChunkDescriptor`.
/// - Memory layout (16 bytes packed):
///   - `first_key`: `u64` (8 bytes)
///   - `payload_rel_offset`: `i32` (4 bytes, scaled $\times 4$, reaching up to $\pm 8\text{ GB}$)
///   - `compressed_len`: `odd_int::U24` (3 bytes, up to $16.7\text{ MB}$)
///   - `row_count`: `u8` (1 byte, logical row count chunk multiplier / count)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
#[cfg_attr(
    feature = "zero-copy",
    derive(
        zerocopy::FromBytes,
        zerocopy::IntoBytes,
        zerocopy::KnownLayout,
        zerocopy::Immutable
    )
)]
pub struct CompactChunkDescriptor {
    /// Initial primary ordering key in this chunk (e.g. `ip_from`, `timestamp`).
    pub first_key: u64,
    /// 4-byte scaled relative displacement to chunk payload in bytes (`offset << 2`).
    /// Reaches up to $\pm 8\text{ GB}$ within container archives.
    pub payload_rel_offset: i32,
    /// Exact compressed length in bytes up to $16.7\text{ MB}$ (`2^{24} - 1`).
    pub compressed_len: odd_int::U24,
    /// Logical row count.
    pub row_count: u8,
}

impl CompactChunkDescriptor {
    pub const SIZE: usize = 16;

    /// Creates a new `CompactChunkDescriptor`.
    pub fn new(
        first_key: u64,
        payload_rel_offset: i32,
        compressed_len: odd_int::U24,
        row_count: u8,
    ) -> Self {
        Self {
            first_key,
            payload_rel_offset,
            compressed_len,
            row_count,
        }
    }

    /// Serializes compact descriptor to bytes.
    pub fn write_to(&self, dst: &mut [u8]) -> Result<(), StitchError> {
        if dst.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: dst.len(),
            });
        }
        dst[0..8].copy_from_slice(&self.first_key.to_le_bytes());
        dst[8..12].copy_from_slice(&self.payload_rel_offset.to_le_bytes());
        dst[12..15].copy_from_slice(&self.compressed_len.to_le_bytes());
        dst[15] = self.row_count;
        Ok(())
    }

    /// Parses compact descriptor from bytes.
    pub fn parse(src: &[u8]) -> Result<Self, StitchError> {
        if src.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: src.len(),
            });
        }
        let first_key = u64::from_le_bytes(src[0..8].try_into().unwrap());
        let payload_rel_offset = i32::from_le_bytes(src[8..12].try_into().unwrap());
        let mut len_bytes = [0u8; 3];
        len_bytes.copy_from_slice(&src[12..15]);
        let compressed_len = unsafe { odd_int::U24::from_le_bytes_unchecked(len_bytes) };
        let row_count = src[15];

        Ok(Self {
            first_key,
            payload_rel_offset,
            compressed_len,
            row_count,
        })
    }
}

/// Fixed-size container file footer (16 bytes).
///
/// Written at the absolute end of the file. Provides index table offset,
/// total chunk count, and Castagnoli CRC32-C verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ContainerFooter {
    /// Byte offset where the chunk descriptor directory begins.
    pub directory_offset: u64,
    /// Number of chunk descriptors in the directory table.
    pub chunk_count: u32,
    /// Castagnoli CRC32-C checksum covering all chunk descriptors.
    pub directory_crc32: u32,
}

impl ContainerFooter {
    pub const SIZE: usize = 16;

    /// Serializes footer to bytes.
    pub fn write_to(&self, dst: &mut [u8]) -> Result<(), StitchError> {
        if dst.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: dst.len(),
            });
        }
        dst[0..8].copy_from_slice(&self.directory_offset.to_le_bytes());
        dst[8..12].copy_from_slice(&self.chunk_count.to_le_bytes());
        dst[12..16].copy_from_slice(&self.directory_crc32.to_le_bytes());
        Ok(())
    }

    /// Parses footer from bytes.
    pub fn parse(src: &[u8]) -> Result<Self, StitchError> {
        if src.len() < Self::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: Self::SIZE,
                available: src.len(),
            });
        }
        Ok(Self {
            directory_offset: u64::from_le_bytes(src[0..8].try_into().unwrap()),
            chunk_count: u32::from_le_bytes(src[8..12].try_into().unwrap()),
            directory_crc32: u32::from_le_bytes(src[12..16].try_into().unwrap()),
        })
    }

    /// Computes CRC32-C of a directory slice.
    pub fn compute_directory_crc(descriptors_bytes: &[u8]) -> u32 {
        crc32_castagnoli(descriptors_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_header_roundtrip() {
        let header = ContainerHeader::new(0x1234_5678_9ABC_DEF0, 0x0001);
        let mut buf = [0u8; 16];
        header.write_to(&mut buf).unwrap();

        let parsed = ContainerHeader::parse(&buf).unwrap();
        assert_eq!(parsed, header);
        assert_eq!(parsed.magic, CONTAINER_MAGIC);
        assert_eq!(parsed.version, CONTAINER_VERSION_1);
    }

    #[test]
    fn test_chunk_descriptor_roundtrip() {
        let desc = ChunkDescriptor {
            first_key: 16_777_216,
            file_offset: 1024,
            compressed_len: 512,
            row_count: 2048,
        };
        let mut buf = [0u8; 24];
        desc.write_to(&mut buf).unwrap();

        let parsed = ChunkDescriptor::parse(&buf).unwrap();
        assert_eq!(parsed, desc);
    }

    #[test]
    fn test_container_footer_roundtrip() {
        let footer = ContainerFooter {
            directory_offset: 65536,
            chunk_count: 32,
            directory_crc32: 0xDEAD_BEEF,
        };
        let mut buf = [0u8; 16];
        footer.write_to(&mut buf).unwrap();

        let parsed = ContainerFooter::parse(&buf).unwrap();
        assert_eq!(parsed, footer);
    }

    #[test]
    fn test_compact_chunk_descriptor_roundtrip() {
        assert_eq!(core::mem::size_of::<CompactChunkDescriptor>(), 16);

        let desc = CompactChunkDescriptor::new(
            16_777_216,
            -256,
            odd_int::U24::from_u64(1024).unwrap(),
            128,
        );
        let mut buf = [0u8; 16];
        desc.write_to(&mut buf).unwrap();

        let parsed = CompactChunkDescriptor::parse(&buf).unwrap();
        assert_eq!(parsed, desc);
        let first_key = parsed.first_key;
        assert_eq!(first_key, 16_777_216);
        let payload_rel_offset = parsed.payload_rel_offset;
        assert_eq!(payload_rel_offset, -256);
        assert_eq!(parsed.compressed_len.to_u64(), 1024);
        assert_eq!(parsed.row_count, 128);
    }
}
