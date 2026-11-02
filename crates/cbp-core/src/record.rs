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
    /// Difference delta encoding.
    Delta,
    /// Lossless IEEE-754 XOR floating point compression (Gorilla).
    Gorilla,
    /// Bounded integer bit-packing with explicit width (1 to 32 bits).
    BitPack(u8),
}

/// Trait implemented by types serializable to and from Compact Binary Protocol representations.
pub trait CbpRecord: Sized {
    /// 64-bit compile-time schema fingerprint computed from struct identifier and field definitions.
    const SCHEMA_HASH: u64;

    /// Encodes a single record into `dst`. Returns the number of bytes written.
    fn encode_record(&self, dst: &mut [u8]) -> Result<usize, CbpError>;

    /// Decodes a single record from `src`. Returns `(record, bytes_consumed)`.
    fn decode_record(src: &[u8]) -> Result<(Self, usize), CbpError>;

    /// Encodes a batch of records into a columnar compressed payload in `dst`.
    ///
    /// Transposes fields into columnar streams and compresses each column using its optimal transform.
    /// Default implementation falls back to sequential record encoding.
    fn encode_batch(records: &[Self], dst: &mut [u8]) -> Result<usize, CbpError> {
        let mut offset = 0;
        let count_len = crate::varint::encode_prefix_varint(records.len() as u64, dst);
        offset += count_len;

        for record in records {
            let written = record.encode_record(&mut dst[offset..])?;
            offset += written;
        }

        Ok(offset)
    }

    /// Decodes a batch of records from a columnar or sequential stream in `src` into `dst`.
    /// Returns the number of records decoded.
    fn decode_batch(src: &[u8], dst: &mut [Self]) -> Result<usize, CbpError> {
        let mut offset = 0;
        let (count, count_len) = crate::varint::decode_prefix_varint(src)?;
        offset += count_len;

        let total = count as usize;
        if dst.len() < total {
            return Err(CbpError::UnexpectedEof {
                required: total,
                available: dst.len(),
            });
        }

        for item in dst[..total].iter_mut() {
            let (record, consumed) = Self::decode_record(&src[offset..])?;
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
        let payload_len = self.encode_record(scratch)?;
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
        let (record, _) = Self::decode_record(payload)?;
        Ok(record)
    }
}
