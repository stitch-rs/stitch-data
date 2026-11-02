//! Record chunk building and batch serialization for `CbpRecord`.
//!
//! Provides cache-conscious batching mechanisms that buffer records in
//! Array-of-Structs (AoS) format and flush framed, profile-optimized CBP packets
//! when target cache-line budgets are reached.

use core::mem::MaybeUninit;

use stitch_data_core::error::CbpError;
use stitch_data_core::frame::{FrameFlags, FrameHeader, OptimizationProfile, pack_frame};
use stitch_data_core::record::StitchRecord;

/// A fixed-capacity, cache-conscious batch builder that buffers records in
/// Array-of-Structs (AoS) format and flushes profile-optimized CBP frames.
pub struct RecordBatchBuilder<T: StitchRecord, const CAPACITY: usize = 256> {
    records: [MaybeUninit<T>; CAPACITY],
    count: usize,
    profile: OptimizationProfile,
}

/// Backward-compatible type alias for `RecordBatchBuilder`.
pub type ColumnarBlockBuilder<T, const CAPACITY: usize = 256> = RecordBatchBuilder<T, CAPACITY>;

impl<T: StitchRecord, const CAPACITY: usize> RecordBatchBuilder<T, CAPACITY> {
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
    /// - `scratch`: Pre-allocated buffer for payload compression.
    /// - `dst`: Pre-allocated buffer for the final framed output packet.
    ///
    /// Returns the number of wire bytes written to `dst`.
    pub fn flush(&mut self, scratch: &mut [u8], dst: &mut [u8]) -> Result<usize, CbpError> {
        if self.count == 0 {
            return Ok(0);
        }

        let slice = self.as_slice();
        let payload_len = T::encode_batch_profile(slice, self.profile, scratch)?;

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

impl<T: StitchRecord, const CAPACITY: usize> Drop for RecordBatchBuilder<T, CAPACITY> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    struct DummyRecord {
        id: u32,
        val: u16,
    }

    impl StitchRecord for DummyRecord {
        const SCHEMA_HASH: u64 = 0x1234_5678_9ABC_DEF0;

        fn encode_record_profile(
            &self,
            _profile: OptimizationProfile,
            dst: &mut [u8],
        ) -> Result<usize, CbpError> {
            dst[0..4].copy_from_slice(&self.id.to_le_bytes());
            dst[4..6].copy_from_slice(&self.val.to_le_bytes());
            Ok(6)
        }

        fn decode_record_profile(
            src: &[u8],
            _profile: OptimizationProfile,
        ) -> Result<(Self, usize), CbpError> {
            let id = u32::from_le_bytes(src[0..4].try_into().unwrap());
            let val = u16::from_le_bytes(src[4..6].try_into().unwrap());
            Ok((Self { id, val }, 6))
        }
    }

    #[test]
    fn test_columnar_block_builder_push_and_flush() {
        let mut builder =
            ColumnarBlockBuilder::<DummyRecord, 4>::new(OptimizationProfile::Balanced);
        assert!(builder.is_empty());

        assert_eq!(builder.push(DummyRecord { id: 1, val: 10 }), Ok(false));
        assert_eq!(builder.push(DummyRecord { id: 2, val: 20 }), Ok(false));
        assert_eq!(builder.push(DummyRecord { id: 3, val: 30 }), Ok(false));
        assert_eq!(builder.push(DummyRecord { id: 4, val: 40 }), Ok(true)); // full!

        assert!(builder.is_full());
        assert_eq!(builder.len(), 4);

        let mut scratch = [0u8; 128];
        let mut dst = [0u8; 256];

        let written = builder
            .flush(&mut scratch, &mut dst)
            .expect("flush must succeed");
        assert!(written > 0);
        assert!(builder.is_empty());

        // Decode batch from flushed frame
        let (header, payload) = stitch_data_core::frame::unpack_frame(&dst[..written]).unwrap();
        assert_eq!(header.schema_hash, Some(DummyRecord::SCHEMA_HASH));
        assert!(header.flags.is_chunked);

        let mut decoded = [DummyRecord { id: 0, val: 0 }; 4];
        let count = DummyRecord::decode_batch(payload, &mut decoded).unwrap();
        assert_eq!(count, 4);
        assert_eq!(decoded[0], DummyRecord { id: 1, val: 10 });
        assert_eq!(decoded[3], DummyRecord { id: 4, val: 40 });
    }
}
