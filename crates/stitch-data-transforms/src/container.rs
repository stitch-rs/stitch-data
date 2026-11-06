//! Container writer and zero-copy block reader for Stitch Data files.
//!
//! Provides `ContainerWriter<W>` for streaming serialized chunked columnar databases,
//! and `ContainerReader<B>` for $O(\log_2 M)$ zero-allocation random block access over `mmap`.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use stitch_data_core::container::{
    ChunkDescriptor, CompactChunkDescriptor, ContainerFooter, ContainerHeader,
    FLAG_COMPACT_DIRECTORY,
};
use stitch_data_core::crc::crc32_castagnoli;
use stitch_data_core::error::StitchError;

/// Chunk descriptor entry in the container writer.
#[derive(Debug, Clone, Copy)]
enum WriterDescriptor {
    Legacy(ChunkDescriptor),
    Compact(CompactChunkDescriptor),
}

/// Stream builder writing a structured Stitch Data container to any `W: std::io::Write`.
#[cfg(feature = "std")]
pub struct ContainerWriter<W: std::io::Write> {
    writer: W,
    current_offset: u64,
    flags: u16,
    descriptors: Vec<WriterDescriptor>,
}

#[cfg(feature = "std")]
impl<W: std::io::Write> ContainerWriter<W> {
    /// Begins a new container with the given `schema_hash` and `flags`.
    pub fn new(mut writer: W, schema_hash: u64, flags: u16) -> Result<Self, StitchError> {
        let header = ContainerHeader::new(schema_hash, flags);
        let mut header_buf = [0u8; ContainerHeader::SIZE];
        header.write_to(&mut header_buf)?;
        writer
            .write_all(&header_buf)
            .map_err(|_| StitchError::PayloadDecodeError)?;

        Ok(Self {
            writer,
            current_offset: ContainerHeader::SIZE as u64,
            flags,
            descriptors: Vec::new(),
        })
    }

    /// Appends an encoded chunk frame to the container file.
    ///
    /// - `first_key`: First primary ordering key in this chunk (for binary search).
    /// - `row_count`: Number of rows in this chunk.
    /// - `chunk_bytes`: Compressed framed payload of the chunk.
    pub fn write_chunk(
        &mut self,
        first_key: u64,
        row_count: usize,
        chunk_bytes: &[u8],
    ) -> Result<(), StitchError> {
        if (self.flags & FLAG_COMPACT_DIRECTORY) != 0 {
            // Scrooge alignment invariant: chunks in compact containers must be 4-byte aligned
            // to allow lossless scaled relative pointers `file_offset = rel_offset << 2`.
            let remainder = self.current_offset % 4;
            if remainder != 0 {
                let pad_len = (4 - remainder) as usize;
                let zeroes = [0u8; 4];
                self.writer
                    .write_all(&zeroes[..pad_len])
                    .map_err(|_| StitchError::PayloadDecodeError)?;
                self.current_offset += pad_len as u64;
            }
        }

        let file_offset = self.current_offset;
        let compressed_len = chunk_bytes.len() as u32;

        self.writer
            .write_all(chunk_bytes)
            .map_err(|_| StitchError::PayloadDecodeError)?;
        self.current_offset += chunk_bytes.len() as u64;

        if (self.flags & FLAG_COMPACT_DIRECTORY) != 0 {
            // For compact descriptor, payload_rel_offset is relative to container header (byte 0).
            // Scaled by 4 (rel_offset = file_offset / 4).
            let payload_rel_offset = (file_offset / 4) as i32;
            let u24_len = odd_int::U24::from_u64(compressed_len as u64)
                .map_err(|_| StitchError::PayloadDecodeError)?;
            let row_count_u8 = if row_count > 255 {
                255u8
            } else {
                row_count as u8
            };

            self.descriptors
                .push(WriterDescriptor::Compact(CompactChunkDescriptor::new(
                    first_key,
                    payload_rel_offset,
                    u24_len,
                    row_count_u8,
                )));
        } else {
            self.descriptors
                .push(WriterDescriptor::Legacy(ChunkDescriptor {
                    first_key,
                    file_offset,
                    compressed_len,
                    row_count: row_count as u32,
                }));
        }

        Ok(())
    }

    /// Finalizes the container by writing the chunk directory and footer.
    ///
    /// Returns total bytes written and chunk count.
    pub fn finish(mut self) -> Result<(u64, usize), StitchError> {
        let directory_offset = self.current_offset;
        let chunk_count = self.descriptors.len() as u32;

        let is_compact = (self.flags & FLAG_COMPACT_DIRECTORY) != 0;
        let desc_size = if is_compact {
            CompactChunkDescriptor::SIZE
        } else {
            ChunkDescriptor::SIZE
        };

        // Serialize chunk descriptors
        let mut dir_buf = Vec::with_capacity(self.descriptors.len() * desc_size);
        for desc in &self.descriptors {
            match desc {
                WriterDescriptor::Legacy(d) => {
                    let mut desc_buf = [0u8; ChunkDescriptor::SIZE];
                    d.write_to(&mut desc_buf)?;
                    dir_buf.extend_from_slice(&desc_buf);
                }
                WriterDescriptor::Compact(d) => {
                    let mut desc_buf = [0u8; CompactChunkDescriptor::SIZE];
                    d.write_to(&mut desc_buf)?;
                    dir_buf.extend_from_slice(&desc_buf);
                }
            }
        }

        let directory_crc32 = crc32_castagnoli(&dir_buf);
        self.writer
            .write_all(&dir_buf)
            .map_err(|_| StitchError::PayloadDecodeError)?;
        self.current_offset += dir_buf.len() as u64;

        // Write footer
        let footer = ContainerFooter {
            directory_offset,
            chunk_count,
            directory_crc32,
        };
        let mut footer_buf = [0u8; ContainerFooter::SIZE];
        footer.write_to(&mut footer_buf)?;
        self.writer
            .write_all(&footer_buf)
            .map_err(|_| StitchError::PayloadDecodeError)?;
        self.current_offset += ContainerFooter::SIZE as u64;

        self.writer
            .flush()
            .map_err(|_| StitchError::PayloadDecodeError)?;

        Ok((self.current_offset, chunk_count as usize))
    }
}

/// Zero-copy container reader operating directly over an in-memory or memory-mapped slice.
pub struct ContainerReader<'a> {
    data: &'a [u8],
    header: ContainerHeader,
    footer: ContainerFooter,
    directory_slice: &'a [u8],
}

impl<'a> ContainerReader<'a> {
    /// Opens and validates a container from a contiguous byte slice.
    pub fn open(data: &'a [u8]) -> Result<Self, StitchError> {
        let min_len = ContainerHeader::SIZE + ContainerFooter::SIZE;
        if data.len() < min_len {
            return Err(StitchError::UnexpectedEof {
                required: min_len,
                available: data.len(),
            });
        }

        // 1. Parse header
        let header = ContainerHeader::parse(&data[..ContainerHeader::SIZE])?;

        // 2. Parse footer from end of buffer
        let footer_start = data.len() - ContainerFooter::SIZE;
        let footer = ContainerFooter::parse(&data[footer_start..])?;

        // 3. Validate directory bounds & CRC
        let dir_start = footer.directory_offset as usize;
        let is_compact = (header.flags & FLAG_COMPACT_DIRECTORY) != 0;
        let desc_size = if is_compact {
            CompactChunkDescriptor::SIZE
        } else {
            ChunkDescriptor::SIZE
        };
        let dir_len = (footer.chunk_count as usize) * desc_size;
        let dir_end = dir_start + dir_len;

        if dir_end > footer_start || dir_start < ContainerHeader::SIZE {
            return Err(StitchError::UnexpectedEof {
                required: dir_end,
                available: footer_start,
            });
        }

        let directory_slice = &data[dir_start..dir_end];
        let actual_crc = crc32_castagnoli(directory_slice);
        if actual_crc != footer.directory_crc32 {
            return Err(StitchError::ChecksumMismatch {
                expected: footer.directory_crc32,
                actual: actual_crc,
            });
        }

        Ok(Self {
            data,
            header,
            footer,
            directory_slice,
        })
    }

    /// Returns the container header.
    #[inline]
    pub fn header(&self) -> &ContainerHeader {
        &self.header
    }

    /// Returns `true` if the container uses 16-byte `CompactChunkDescriptor` directory layout.
    #[inline]
    pub fn is_compact(&self) -> bool {
        (self.header.flags & FLAG_COMPACT_DIRECTORY) != 0
    }

    /// Returns total chunk count.
    #[inline]
    pub fn chunk_count(&self) -> usize {
        self.footer.chunk_count as usize
    }

    /// Returns the byte offset within the container where the chunk directory begins.
    #[inline]
    pub fn directory_offset(&self) -> u64 {
        self.footer.directory_offset
    }

    /// Returns the container footer.
    #[inline]
    pub fn footer(&self) -> &ContainerFooter {
        &self.footer
    }

    /// Retrieves a chunk descriptor by index.
    ///
    /// Transparently normalizes 16-byte `CompactChunkDescriptor` to `ChunkDescriptor` if compact flag is set.
    pub fn chunk_descriptor(&self, index: usize) -> Result<ChunkDescriptor, StitchError> {
        if index >= self.chunk_count() {
            return Err(StitchError::PayloadDecodeError);
        }
        if self.is_compact() {
            let offset = index * CompactChunkDescriptor::SIZE;
            let compact = CompactChunkDescriptor::parse(
                &self.directory_slice[offset..offset + CompactChunkDescriptor::SIZE],
            )?;
            let file_offset = ((compact.payload_rel_offset as i64) * 4) as u64;
            Ok(ChunkDescriptor {
                first_key: compact.first_key,
                file_offset,
                compressed_len: compact.compressed_len.to_u64() as u32,
                row_count: compact.row_count as u32,
            })
        } else {
            let offset = index * ChunkDescriptor::SIZE;
            ChunkDescriptor::parse(&self.directory_slice[offset..offset + ChunkDescriptor::SIZE])
        }
    }

    /// Retrieves a native 16-byte `CompactChunkDescriptor` by index.
    ///
    /// Fails with `PayloadDecodeError` if container was not written with `FLAG_COMPACT_DIRECTORY`.
    pub fn compact_chunk_descriptor(
        &self,
        index: usize,
    ) -> Result<CompactChunkDescriptor, StitchError> {
        if !self.is_compact() || index >= self.chunk_count() {
            return Err(StitchError::PayloadDecodeError);
        }
        let offset = index * CompactChunkDescriptor::SIZE;
        CompactChunkDescriptor::parse(
            &self.directory_slice[offset..offset + CompactChunkDescriptor::SIZE],
        )
    }

    /// Retrieves the raw compressed byte slice of a chunk by descriptor.
    pub fn chunk_slice(&self, desc: &ChunkDescriptor) -> Result<&'a [u8], StitchError> {
        let start = desc.file_offset as usize;
        let end = start + (desc.compressed_len as usize);
        if end > self.data.len() {
            return Err(StitchError::UnexpectedEof {
                required: end,
                available: self.data.len(),
            });
        }
        Ok(&self.data[start..end])
    }

    /// $O(\log_2 M)$ branchless binary search to locate the candidate chunk for `key`.
    ///
    /// Finds the greatest chunk whose `first_key <= key`.
    pub fn find_chunk_for_key(&self, key: u64) -> Option<ChunkDescriptor> {
        let count = self.chunk_count();
        if count == 0 {
            return None;
        }

        let mut low = 0;
        let mut high = count;
        let mut candidate = None;

        while low < high {
            let mid = low + (high - low) / 2;
            let desc = self.chunk_descriptor(mid).ok()?;

            if desc.first_key <= key {
                candidate = Some(desc);
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "std")]
    fn test_container_writer_and_reader_roundtrip() {
        let mut output = Vec::new();
        let schema_hash = 0xCAFE_BABE_0001;

        let mut writer = ContainerWriter::new(&mut output, schema_hash, 0).unwrap();

        // Write 3 mock chunk blocks
        let chunk0 = b"block_0_payload_content_12345";
        let chunk1 = b"block_1_payload_content_67890";
        let chunk2 = b"block_2_payload_content_ABCDEF";

        writer.write_chunk(100, 10, chunk0).unwrap();
        writer.write_chunk(200, 10, chunk1).unwrap();
        writer.write_chunk(300, 10, chunk2).unwrap();

        let (total_len, chunks_written) = writer.finish().unwrap();
        assert_eq!(chunks_written, 3);
        assert_eq!(total_len as usize, output.len());

        // Open reader
        let reader = ContainerReader::open(&output).unwrap();
        assert_eq!(reader.header().schema_hash, schema_hash);
        assert_eq!(reader.chunk_count(), 3);

        let d0 = reader.chunk_descriptor(0).unwrap();
        assert_eq!(d0.first_key, 100);
        assert_eq!(reader.chunk_slice(&d0).unwrap(), chunk0);

        let d1 = reader.chunk_descriptor(1).unwrap();
        assert_eq!(d1.first_key, 200);
        assert_eq!(reader.chunk_slice(&d1).unwrap(), chunk1);

        // Binary search lookup
        let found150 = reader.find_chunk_for_key(150).unwrap();
        assert_eq!(found150.first_key, 100);

        let found250 = reader.find_chunk_for_key(250).unwrap();
        assert_eq!(found250.first_key, 200);

        let found999 = reader.find_chunk_for_key(999).unwrap();
        assert_eq!(found999.first_key, 300);

        let found50 = reader.find_chunk_for_key(50);
        assert!(found50.is_none(), "Key before first chunk must return None");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_compact_container_writer_and_reader_roundtrip() {
        let mut output = Vec::new();
        let schema_hash = 0xBEEF_CAFE_0002;

        // Container with FLAG_COMPACT_DIRECTORY
        let mut writer =
            ContainerWriter::new(&mut output, schema_hash, FLAG_COMPACT_DIRECTORY).unwrap();

        let chunk0 = b"payload_compact_chunk_00000000"; // 31 bytes -> let's pad to multiple of 4
        let chunk1 = b"payload_compact_chunk_11111111"; // len = 31 bytes! Notice 31 % 4 != 0
        let chunk2 = b"payload_compact_chunk_22222222";

        writer.write_chunk(1_000, 50, chunk0).unwrap();
        writer.write_chunk(2_000, 60, chunk1).unwrap();
        writer.write_chunk(3_000, 70, chunk2).unwrap();

        let (total_len, chunks_written) = writer.finish().unwrap();
        assert_eq!(chunks_written, 3);
        assert_eq!(total_len as usize, output.len());

        // Open reader
        let reader = ContainerReader::open(&output).unwrap();
        assert!(reader.is_compact());
        assert_eq!(reader.header().schema_hash, schema_hash);
        assert_eq!(reader.chunk_count(), 3);

        // Native 16-byte CompactChunkDescriptor check
        let c0 = reader.compact_chunk_descriptor(0).unwrap();
        let c0_key = c0.first_key;
        assert_eq!(c0_key, 1_000);
        assert_eq!(c0.row_count, 50);
        assert_eq!(c0.compressed_len.to_u64(), chunk0.len() as u64);

        // Transparent ChunkDescriptor lookup & slice read
        let d0 = reader.chunk_descriptor(0).unwrap();
        assert_eq!(d0.first_key, 1_000);
        assert_eq!(d0.row_count, 50);
        assert_eq!(reader.chunk_slice(&d0).unwrap(), chunk0);

        let d1 = reader.chunk_descriptor(1).unwrap();
        assert_eq!(d1.first_key, 2_000);
        assert_eq!(d1.row_count, 60);
        assert_eq!(reader.chunk_slice(&d1).unwrap(), chunk1);

        let d2 = reader.chunk_descriptor(2).unwrap();
        assert_eq!(d2.first_key, 3_000);
        assert_eq!(d2.row_count, 70);
        assert_eq!(reader.chunk_slice(&d2).unwrap(), chunk2);

        // Binary search lookup
        let found = reader.find_chunk_for_key(2500).unwrap();
        assert_eq!(found.first_key, 2000);
        assert_eq!(reader.chunk_slice(&found).unwrap(), chunk1);
    }
}
