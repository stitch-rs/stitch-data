//! High-level record traits and codec metadata.

use crate::error::CbpError;
use crate::frame::{FrameFlags, FrameHeader, OptimizationProfile, pack_frame, unpack_frame};

/// Explicit transform codec for a record field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldCodec {
    /// Raw little-endian bytes without transformations.
    Raw,
    /// Branchless PrefixVarint encoding.
    Varint,
    /// Signed ZigZag variable-length integer.
    ZigZag,
    /// Bounded integer bit-packing with explicit width (1 to 64 bits).
    BitPack(u8),
}

/// Trait implemented by types serializable to and from Compact Binary Protocol representations.
pub trait CbpRecord: Sized {
    /// 64-bit compile-time schema fingerprint computed from struct identifier and field definitions.
    const SCHEMA_HASH: u64;

    /// Encodes a single record into `dst` using the default Balanced optimization profile.
    /// Returns the number of bytes written.
    #[inline]
    fn encode_record(&self, dst: &mut [u8]) -> Result<usize, CbpError> {
        self.encode_record_profile(OptimizationProfile::Balanced, dst)
    }

    /// Decodes a single record from `src` using the default Balanced optimization profile.
    /// Returns `(record, bytes_consumed)`.
    #[inline]
    fn decode_record(src: &[u8]) -> Result<(Self, usize), CbpError> {
        Self::decode_record_profile(src, OptimizationProfile::Balanced)
    }

    /// Encodes a single record into `dst` dispatched according to the specified `OptimizationProfile`.
    ///
    /// - `OptimizationProfile::Direct`: Serializes fields as raw uncompressed little-endian POD bytes.
    /// - `OptimizationProfile::Realtime` / `Balanced` / `MaxDensity`: Applies variable-length and bit-packed codecs.
    fn encode_record_profile(
        &self,
        profile: OptimizationProfile,
        dst: &mut [u8],
    ) -> Result<usize, CbpError>;

    /// Decodes a single record from `src` dispatched according to the specified `OptimizationProfile`.
    fn decode_record_profile(
        src: &[u8],
        profile: OptimizationProfile,
    ) -> Result<(Self, usize), CbpError>;

    /// Encodes a batch of records into a payload in `dst` with the default profile.
    #[inline]
    fn encode_batch(records: &[Self], dst: &mut [u8]) -> Result<usize, CbpError> {
        Self::encode_batch_profile(records, OptimizationProfile::Balanced, dst)
    }

    /// Encodes a batch of records into a payload in `dst` with the given optimization profile.
    fn encode_batch_profile(
        records: &[Self],
        profile: OptimizationProfile,
        dst: &mut [u8],
    ) -> Result<usize, CbpError> {
        if dst.len() < 9 {
            return Err(CbpError::UnexpectedEof {
                required: 9,
                available: dst.len(),
            });
        }
        let mut offset = 0;
        let count_len = crate::varint::encode_prefix_varint(records.len() as u64, dst);
        offset += count_len;

        for record in records {
            let written = record.encode_record_profile(profile, &mut dst[offset..])?;
            offset += written;
        }

        Ok(offset)
    }

    /// Decodes a batch of records from a stream in `src` into `dst` with the default profile.
    #[inline]
    fn decode_batch(src: &[u8], dst: &mut [Self]) -> Result<usize, CbpError> {
        Self::decode_batch_profile(src, OptimizationProfile::Balanced, dst)
    }

    /// Decodes a batch of records from a stream in `src` into `dst` with the given optimization profile.
    fn decode_batch_profile(
        src: &[u8],
        profile: OptimizationProfile,
        dst: &mut [Self],
    ) -> Result<usize, CbpError> {
        let mut offset = 0;
        let (count, count_len) = crate::varint::decode_prefix_varint(src)?;
        offset += count_len;

        let total = usize::try_from(count).map_err(|_| CbpError::PayloadDecodeError)?;
        if dst.len() < total {
            return Err(CbpError::UnexpectedEof {
                required: total,
                available: dst.len(),
            });
        }

        for item in dst[..total].iter_mut() {
            let (record, consumed) = Self::decode_record_profile(&src[offset..], profile)?;
            *item = record;
            offset += consumed;
        }

        Ok(total)
    }

    /// Packs a single record into a framed CBP envelope with schema validation header.
    fn pack_to_frame(
        &self,
        profile: OptimizationProfile,
        scratch: &mut [u8],
        dst: &mut [u8],
    ) -> Result<usize, CbpError> {
        let payload_len = self.encode_record_profile(profile, scratch)?;
        let header = FrameHeader {
            profile,
            flags: FrameFlags {
                is_compressed: profile != OptimizationProfile::Direct,
                is_chunked: false,
                has_schema_hash: true,
            },
            payload_len,
            schema_hash: Some(Self::SCHEMA_HASH),
        };
        pack_frame(&header, &scratch[..payload_len], dst)
    }

    /// Unpacks a single record from a framed CBP envelope, verifying schema fingerprint and CRC32.
    ///
    /// Dispatches parsing using `header.profile` and rejects trailing unconsumed bytes.
    fn unpack_from_frame(src: &[u8]) -> Result<Self, CbpError> {
        let (header, payload) = unpack_frame(src)?;
        if header.flags.has_schema_hash
            && let Some(actual) = header.schema_hash
            && actual != Self::SCHEMA_HASH
        {
            return Err(CbpError::SchemaMismatch {
                expected: Self::SCHEMA_HASH,
                actual,
            });
        }
        let (record, consumed) = Self::decode_record_profile(payload, header.profile)?;
        if consumed != payload.len() {
            return Err(CbpError::PayloadDecodeError);
        }
        Ok(record)
    }
}
