//! True Columnar (SoA) block building and batch serialization for `Record`.
//!
//! Provides zero-allocation columnar transposition and streaming codecs that buffer
//! records in Array-of-Structs (AoS) format and flush framed, columnar-compressed
//! Structure-of-Arrays (SoA) packets for maximum compression ratio.

use core::mem::MaybeUninit;

use stitch_data_core::error::StitchError;
use stitch_data_core::frame::{
    FrameFlags, FrameHeader, OptimizationProfile, pack_frame, unpack_frame,
};
use stitch_data_core::record::Record;

/// Tag identifying the specific columnar encoding algorithm used for a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ColumnCodecTag {
    /// Raw uncompressed bytes.
    Raw = 0x00,
    /// Adaptive unsigned integer column (Delta + BitPacking or RLE).
    AdaptiveU32 = 0x01,
    /// Signed integer column (Delta + ZigZag + BitPacking).
    DeltaZigZagI32 = 0x02,
    /// Lossless IEEE-754 float XOR delta (Gorilla).
    GorillaF32 = 0x03,
    /// Run-Length Encoding for repetitive sequences.
    Rle = 0x04,
    /// Bounded zero-allocation dictionary encoding for categorical symbols.
    Dictionary = 0x05,
}

impl ColumnCodecTag {
    /// Converts a byte tag into a `ColumnCodecTag`.
    pub const fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0x00 => Some(Self::Raw),
            0x01 => Some(Self::AdaptiveU32),
            0x02 => Some(Self::DeltaZigZagI32),
            0x03 => Some(Self::GorillaF32),
            0x04 => Some(Self::Rle),
            0x05 => Some(Self::Dictionary),
            _ => None,
        }
    }
}

/// Zero-allocation columnar block encoder building self-describing SoA blocks.
pub struct ColumnarBlockWriter<'a> {
    dst: &'a mut [u8],
    offset: usize,
    num_columns: usize,
    columns_written: usize,
}

impl<'a> ColumnarBlockWriter<'a> {
    /// Begins a new columnar block with `row_count` rows and `num_columns` columns.
    pub fn new(
        row_count: usize,
        num_columns: usize,
        dst: &'a mut [u8],
    ) -> Result<Self, StitchError> {
        if dst.len() < 18 {
            return Err(StitchError::UnexpectedEof {
                required: 18,
                available: dst.len(),
            });
        }
        let mut offset = 0;
        let rlen = stitch_data_core::varint::encode_prefix_varint(row_count as u64, dst);
        offset += rlen;

        let clen =
            stitch_data_core::varint::encode_prefix_varint(num_columns as u64, &mut dst[offset..]);
        offset += clen;

        Ok(Self {
            dst,
            offset,
            num_columns,
            columns_written: 0,
        })
    }

    /// Writes a compressed column payload into the block.
    pub fn write_column(
        &mut self,
        codec: ColumnCodecTag,
        payload: &[u8],
    ) -> Result<(), StitchError> {
        if self.columns_written >= self.num_columns {
            return Err(StitchError::PayloadDecodeError);
        }

        if self.dst.len() < self.offset + 10 {
            return Err(StitchError::UnexpectedEof {
                required: self.offset + 10,
                available: self.dst.len(),
            });
        }
        self.dst[self.offset] = codec as u8;
        self.offset += 1;

        let len_bytes = stitch_data_core::varint::encode_prefix_varint(
            payload.len() as u64,
            &mut self.dst[self.offset..],
        );
        self.offset += len_bytes;

        if self.dst.len() < self.offset + payload.len() {
            return Err(StitchError::UnexpectedEof {
                required: self.offset + payload.len(),
                available: self.dst.len(),
            });
        }
        self.dst[self.offset..self.offset + payload.len()].copy_from_slice(payload);
        self.offset += payload.len();
        self.columns_written += 1;
        Ok(())
    }

    /// Finalizes the block and returns total bytes written.
    pub fn finish(self) -> Result<usize, StitchError> {
        if self.columns_written != self.num_columns {
            return Err(StitchError::PayloadDecodeError);
        }
        Ok(self.offset)
    }
}

/// Zero-allocation columnar block decoder parsing SoA column streams.
pub struct ColumnarBlockReader<'a> {
    src: &'a [u8],
    offset: usize,
    row_count: usize,
    num_columns: usize,
    columns_read: usize,
}

impl<'a> ColumnarBlockReader<'a> {
    /// Parses the header of a columnar block.
    pub fn new(src: &'a [u8]) -> Result<Self, StitchError> {
        let mut offset = 0;
        let (rc, rlen) = stitch_data_core::varint::decode_prefix_varint(src)?;
        offset += rlen;
        let (cc, clen) = stitch_data_core::varint::decode_prefix_varint(&src[offset..])?;
        offset += clen;

        let row_count = usize::try_from(rc).map_err(|_| StitchError::PayloadDecodeError)?;
        let num_columns = usize::try_from(cc).map_err(|_| StitchError::PayloadDecodeError)?;

        Ok(Self {
            src,
            offset,
            row_count,
            num_columns,
            columns_read: 0,
        })
    }

    /// Returns the number of rows contained in this block.
    #[inline]
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// Returns the number of columns in this block.
    #[inline]
    pub const fn num_columns(&self) -> usize {
        self.num_columns
    }

    /// Returns the current byte offset within the source buffer.
    #[inline]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Reads the next column descriptor and borrowed compressed payload slice.
    pub fn next_column(&mut self) -> Result<(ColumnCodecTag, &'a [u8]), StitchError> {
        if self.columns_read >= self.num_columns {
            return Err(StitchError::PayloadDecodeError);
        }

        if self.src.len() < self.offset + 1 {
            return Err(StitchError::UnexpectedEof {
                required: self.offset + 1,
                available: self.src.len(),
            });
        }
        let tag_byte = self.src[self.offset];
        let tag = ColumnCodecTag::from_u8(tag_byte).ok_or(StitchError::PayloadDecodeError)?;
        self.offset += 1;

        let (col_len_u64, len_bytes) =
            stitch_data_core::varint::decode_prefix_varint(&self.src[self.offset..])?;
        self.offset += len_bytes;
        let col_len = usize::try_from(col_len_u64).map_err(|_| StitchError::PayloadDecodeError)?;

        if self.src.len() < self.offset + col_len {
            return Err(StitchError::UnexpectedEof {
                required: self.offset + col_len,
                available: self.src.len(),
            });
        }
        let payload = &self.src[self.offset..self.offset + col_len];
        self.offset += col_len;
        self.columns_read += 1;
        Ok((tag, payload))
    }
}

// --- Columnar Codec Encoding & Decoding Primitives ---

/// Encodes an unsigned 32-bit integer column slice into `dst` using adaptive Delta+BitPack or RLE.
pub fn encode_u32_column(
    slice: &[u32],
    scratch: &mut [u32],
    dst: &mut [u8],
) -> Result<usize, StitchError> {
    let mut pipeline = crate::pipeline::CompressionPipeline::new();
    pipeline.compress_max_density_u32(slice, scratch, dst)
}

/// Decodes an unsigned 32-bit integer column from `src` into `dst`.
pub fn decode_u32_column(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
    crate::pipeline::DecompressionPipeline::decompress_max_density_u32(src, dst)
}

/// Encodes a signed 32-bit integer column slice into `dst` using Delta + ZigZag + BitPack.
pub fn encode_i32_column(
    slice: &[i32],
    scratch: &mut [u32],
    dst: &mut [u8],
) -> Result<usize, StitchError> {
    crate::pipeline::CompressionPipeline::compress_i32(slice, scratch, dst)
}

/// Decodes a signed 32-bit integer column from `src` into `dst`.
pub fn decode_i32_column(
    src: &[u8],
    scratch: &mut [u32],
    dst: &mut [i32],
) -> Result<usize, StitchError> {
    crate::pipeline::DecompressionPipeline::decompress_i32(src, scratch, dst)
}

/// Encodes an IEEE-754 32-bit float column slice into `dst` using Gorilla XOR delta compression.
pub fn encode_f32_column(slice: &[f32], dst: &mut [u8]) -> Result<usize, StitchError> {
    crate::float::GorillaCompressor::compress_f32(slice, dst)
}

/// Decodes an IEEE-754 32-bit float column from `src` into `dst`.
pub fn decode_f32_column(src: &[u8], dst: &mut [f32]) -> Result<usize, StitchError> {
    crate::float::GorillaCompressor::decompress_f32(src, dst)
}

/// Encodes raw POD bytes without transformation.
pub fn encode_raw_column(bytes: &[u8], dst: &mut [u8]) -> Result<usize, StitchError> {
    if dst.len() < bytes.len() {
        return Err(StitchError::UnexpectedEof {
            required: bytes.len(),
            available: dst.len(),
        });
    }
    dst[..bytes.len()].copy_from_slice(bytes);
    Ok(bytes.len())
}

/// Decodes raw POD bytes.
pub fn decode_raw_column(src: &[u8], dst: &mut [u8]) -> Result<usize, StitchError> {
    if src.len() < dst.len() {
        return Err(StitchError::UnexpectedEof {
            required: dst.len(),
            available: src.len(),
        });
    }
    dst.copy_from_slice(&src[..dst.len()]);
    Ok(dst.len())
}

/// Encodes a slice of `u32` into `dst` using Run-Length Encoding.
pub fn encode_rle_column(slice: &[u32], dst: &mut [u8]) -> Result<usize, StitchError> {
    crate::rle::RleTransformer::encode_u32(slice, dst)
}

/// Decodes an RLE-encoded sequence from `src` into `dst`.
///
/// Returns the number of elements written to `dst`.
pub fn decode_rle_column(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
    let (elements, _bytes) = crate::rle::RleTransformer::decode_u32_ext(src, dst)?;
    Ok(elements)
}

/// Encodes a categorical slice of `u32` symbols into `dst` using bounded Dictionary encoding.
pub fn encode_dict_column(slice: &[u32], dst: &mut [u8]) -> Result<usize, StitchError> {
    crate::dict::DictTransformer::encode_u32(slice, dst)
}

/// Decodes a dictionary-compressed column from `src` into `dst`.
///
/// Returns the number of elements written to `dst`.
pub fn decode_dict_column(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
    let (elements, _bytes) = crate::dict::DictTransformer::decode_u32(src, dst)?;
    Ok(elements)
}

// --- Batch Builder & Slicing ---

/// A fixed-capacity, cache-conscious batch builder that buffers records in
/// Array-of-Structs (AoS) format and flushes framed, profile-optimized CBP packets.
pub struct RecordBatchBuilder<T: Record, const CAPACITY: usize = 256> {
    records: [MaybeUninit<T>; CAPACITY],
    count: usize,
    profile: OptimizationProfile,
}

/// Backward-compatible type alias for `RecordBatchBuilder`.
pub type ColumnarBlockBuilder<T, const CAPACITY: usize = 256> = RecordBatchBuilder<T, CAPACITY>;

impl<T: Record, const CAPACITY: usize> RecordBatchBuilder<T, CAPACITY> {
    /// Creates a new `RecordBatchBuilder` with the specified optimization profile.
    pub const fn new(profile: OptimizationProfile) -> Self {
        // SAFETY: An uninitialized array of MaybeUninit is valid.
        let records: [MaybeUninit<T>; CAPACITY] = unsafe { MaybeUninit::uninit().assume_init() };
        Self {
            records,
            count: 0,
            profile,
        }
    }

    /// Returns the number of buffered records.
    #[inline]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no records are buffered.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns `true` if the buffer is at maximum capacity.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.count >= CAPACITY
    }

    /// Appends a record to the buffer.
    ///
    /// - Returns `Ok(true)` if the buffer is full and should be flushed.
    /// - Returns `Ok(false)` if the buffer has remaining space.
    /// - Returns `Err(record)` if the buffer was already at full capacity.
    pub fn push(&mut self, record: T) -> Result<bool, T> {
        if self.count >= CAPACITY {
            return Err(record);
        }
        self.records[self.count].write(record);
        self.count += 1;
        Ok(self.count >= CAPACITY)
    }

    /// Returns a borrowed slice of the initialized records.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: self.records[..self.count] are all initialized.
        unsafe { core::slice::from_raw_parts(self.records.as_ptr() as *const T, self.count) }
    }

    /// Flushes all buffered records into a framed CBP packet in `dst`.
    ///
    /// When `self.profile == OptimizationProfile::MaxDensity`, true columnar SoA block
    /// compression is automatically utilized.
    pub fn flush(&mut self, scratch: &mut [u8], dst: &mut [u8]) -> Result<usize, StitchError> {
        if self.count == 0 {
            return Ok(0);
        }

        let slice = self.as_slice();
        let payload_len = if self.profile == OptimizationProfile::MaxDensity {
            T::encode_columnar_batch(slice, scratch)?
        } else {
            T::encode_batch_profile(slice, self.profile, scratch)?
        };

        let header = FrameHeader {
            profile: self.profile,
            flags: FrameFlags {
                is_compressed: self.profile != OptimizationProfile::Direct,
                is_chunked: true,
                has_schema_hash: true,
            },
            payload_len,
            schema_hash: Some(T::SCHEMA_HASH),
        };

        let total_written = pack_frame(&header, &scratch[..payload_len], dst)?;

        // Drop initialized items if needed and reset count
        self.clear();

        Ok(total_written)
    }

    /// Explicitly flushes buffered records as a true Columnar (Structure-of-Arrays) framed packet.
    pub fn flush_columnar(
        &mut self,
        scratch: &mut [u8],
        dst: &mut [u8],
    ) -> Result<usize, StitchError> {
        if self.count == 0 {
            return Ok(0);
        }

        let slice = self.as_slice();
        let payload_len = T::encode_columnar_batch(slice, scratch)?;

        let header = FrameHeader {
            profile: OptimizationProfile::MaxDensity,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: true,
                has_schema_hash: true,
            },
            payload_len,
            schema_hash: Some(T::SCHEMA_HASH),
        };

        let total_written = pack_frame(&header, &scratch[..payload_len], dst)?;
        self.clear();
        Ok(total_written)
    }

    /// Clears all buffered records without flushing.
    pub fn clear(&mut self) {
        if core::mem::needs_drop::<T>() {
            for i in 0..self.count {
                // SAFETY: self.records[i] was initialized.
                unsafe {
                    self.records[i].assume_init_drop();
                }
            }
        }
        self.count = 0;
    }
}

impl<T: Record, const CAPACITY: usize> Drop for RecordBatchBuilder<T, CAPACITY> {
    fn drop(&mut self) {
        self.clear();
    }
}

// --- Column Slicing & Random Access Primitives ---

/// Borrowed column slice descriptor containing its codec tag and payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnSlice<'a> {
    /// Column encoding codec tag.
    pub codec: ColumnCodecTag,
    /// Borrowed raw column bytes.
    pub payload: &'a [u8],
}

/// Zero-allocation, cache-friendly random-access index for a columnar block.
///
/// Pre-indexes column offsets in $O(K)$ time where $K$ is the number of columns,
/// allowing subsequent $O(1)$ random-access lookups to any column and fast
/// single-value extraction without decoding full blocks.
pub struct ColumnSliceIndex<'a, const MAX_COLUMNS: usize = 16> {
    row_count: usize,
    num_columns: usize,
    columns: [Option<ColumnSlice<'a>>; MAX_COLUMNS],
}

impl<'a, const MAX_COLUMNS: usize> ColumnSliceIndex<'a, MAX_COLUMNS> {
    /// Indexes a serialized columnar block for $O(1)$ column access.
    pub fn new(src: &'a [u8]) -> Result<Self, StitchError> {
        let mut reader = ColumnarBlockReader::new(src)?;
        let row_count = reader.row_count();
        let num_columns = reader.num_columns();

        if num_columns > MAX_COLUMNS {
            return Err(StitchError::PayloadDecodeError);
        }

        let mut columns = [None; MAX_COLUMNS];
        for col_slot in columns.iter_mut().take(num_columns) {
            let (codec, payload) = reader.next_column()?;
            *col_slot = Some(ColumnSlice { codec, payload });
        }

        Ok(Self {
            row_count,
            num_columns,
            columns,
        })
    }

    /// Returns the number of rows contained in this block.
    #[inline(always)]
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// Returns the number of columns in this block.
    #[inline(always)]
    pub const fn num_columns(&self) -> usize {
        self.num_columns
    }

    /// Returns the column descriptor and payload slice for `column_idx` in $O(1)$ time.
    #[inline(always)]
    pub fn column(&self, column_idx: usize) -> Option<ColumnSlice<'a>> {
        if column_idx < self.num_columns {
            self.columns[column_idx]
        } else {
            None
        }
    }

    /// Reads a raw POD value of type `T` from a `Raw` column at `row_idx` in $O(1)$ time.
    pub fn read_raw_at<T: Copy>(
        &self,
        column_idx: usize,
        row_idx: usize,
    ) -> Result<T, StitchError> {
        if row_idx >= self.row_count {
            return Err(StitchError::PayloadDecodeError);
        }
        let col = self
            .column(column_idx)
            .ok_or(StitchError::PayloadDecodeError)?;
        if col.codec != ColumnCodecTag::Raw {
            return Err(StitchError::PayloadDecodeError);
        }

        let item_size = core::mem::size_of::<T>();
        let offset = row_idx
            .checked_mul(item_size)
            .ok_or(StitchError::PayloadDecodeError)?;
        if col.payload.len() < offset + item_size {
            return Err(StitchError::UnexpectedEof {
                required: offset + item_size,
                available: col.payload.len(),
            });
        }

        // SAFETY: Pointer is within bounds of col.payload and T is Copy.
        // We use read_unaligned in case byte alignment is not guaranteed.
        unsafe {
            let ptr = col.payload.as_ptr().add(offset) as *const T;
            Ok(core::ptr::read_unaligned(ptr))
        }
    }
}

/// Reads a single unpacked `u32` value directly from a bitpacked column payload at `row_idx` in $O(1)$ time.
#[inline(always)]
pub fn read_bitpacked_u32_at(
    payload: &[u8],
    row_idx: usize,
    total_count: usize,
) -> Result<u32, StitchError> {
    crate::bitpack::BitPacker::read_at(payload, row_idx, total_count)
}

/// Decodes a framed columnar batch packet into `dst`.
pub fn decode_columnar_frame<T: Record>(frame: &[u8], dst: &mut [T]) -> Result<usize, StitchError> {
    let (header, payload) = unpack_frame(frame)?;
    if let Some(hash) = header.schema_hash
        && hash != T::SCHEMA_HASH
    {
        return Err(StitchError::SchemaMismatch {
            expected: T::SCHEMA_HASH,
            actual: hash,
        });
    }
    T::decode_columnar_batch(payload, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_slice_index_and_random_access() {
        let mut buf = [0u8; 512];
        let mut writer = ColumnarBlockWriter::new(4, 2, &mut buf).unwrap();

        // Col 0: Bitpacked u32 (3-bit values: 0..7)
        let col0_raw = [3u32, 7, 1, 5];
        let mut col0_packed = [0u8; 64];
        let col0_len = crate::bitpack::BitPacker::pack_u32(&col0_raw, &mut col0_packed).unwrap();
        writer
            .write_column(ColumnCodecTag::AdaptiveU32, &col0_packed[..col0_len])
            .unwrap();

        // Col 1: Raw f64 values
        let col1_raw = [10.5f64, 20.25, 30.125, 40.0625];
        let col1_bytes = unsafe {
            core::slice::from_raw_parts(
                col1_raw.as_ptr() as *const u8,
                core::mem::size_of_val(&col1_raw),
            )
        };
        let mut col1_buf = [0u8; 64];
        let col1_len = encode_raw_column(col1_bytes, &mut col1_buf).unwrap();
        writer
            .write_column(ColumnCodecTag::Raw, &col1_buf[..col1_len])
            .unwrap();

        let total_written = writer.finish().unwrap();

        // Build index in O(K) time
        let index = ColumnSliceIndex::<4>::new(&buf[..total_written]).unwrap();
        assert_eq!(index.row_count(), 4);
        assert_eq!(index.num_columns(), 2);

        // O(1) Raw column read
        for (i, &expected) in col1_raw.iter().enumerate() {
            let actual: f64 = index.read_raw_at(1, i).unwrap();
            assert_eq!(actual, expected);
        }

        // O(1) Bitpacked column read
        let col0_slice = index.column(0).unwrap();
        assert_eq!(col0_slice.codec, ColumnCodecTag::AdaptiveU32);
        for (i, &expected) in col0_raw.iter().enumerate() {
            let actual = read_bitpacked_u32_at(col0_slice.payload, i, index.row_count()).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn test_columnar_block_writer_and_reader() {
        let mut buf = [0u8; 512];
        let mut writer = ColumnarBlockWriter::new(4, 2, &mut buf).unwrap();

        let col0_raw = [10u32, 20, 30, 40];
        let mut scratch = [0u32; 4];
        let mut col0_compressed = [0u8; 64];
        let col0_len = encode_u32_column(&col0_raw, &mut scratch, &mut col0_compressed).unwrap();
        writer
            .write_column(ColumnCodecTag::AdaptiveU32, &col0_compressed[..col0_len])
            .unwrap();

        let col1_raw = [-5i32, -4, -3, -2];
        let mut col1_compressed = [0u8; 64];
        let col1_len = encode_i32_column(&col1_raw, &mut scratch, &mut col1_compressed).unwrap();
        writer
            .write_column(ColumnCodecTag::DeltaZigZagI32, &col1_compressed[..col1_len])
            .unwrap();

        let total_written = writer.finish().unwrap();

        // Read back
        let mut reader = ColumnarBlockReader::new(&buf[..total_written]).unwrap();
        assert_eq!(reader.row_count(), 4);
        assert_eq!(reader.num_columns(), 2);

        // Decode col 0
        let (tag0, payload0) = reader.next_column().unwrap();
        assert_eq!(tag0, ColumnCodecTag::AdaptiveU32);
        let mut decoded_u32 = [0u32; 4];
        decode_u32_column(payload0, &mut decoded_u32).unwrap();
        assert_eq!(decoded_u32, col0_raw);

        // Decode col 1
        let (tag1, payload1) = reader.next_column().unwrap();
        assert_eq!(tag1, ColumnCodecTag::DeltaZigZagI32);
        let mut decoded_i32 = [0i32; 4];
        decode_i32_column(payload1, &mut scratch, &mut decoded_i32).unwrap();
        assert_eq!(decoded_i32, col1_raw);
    }
}
