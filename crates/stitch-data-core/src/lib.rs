//! Core `#![no_std]` framing, bitstream, PrefixVarint, and ZigZag primitives.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bitstream;
pub mod crc;
pub mod error;
pub mod frame;
pub mod record;
pub mod resync;
pub mod simd;
pub mod varint;
pub mod zigzag;

pub use crc::{
    crc32_castagnoli, crc32_castagnoli_finalize, crc32_castagnoli_init, crc32_castagnoli_slices,
    crc32_castagnoli_update,
};
pub use error::{CbpError, StitchError};
pub use frame::{
    CBP_MAGIC, CBP_VERSION_1, FrameFlags, FrameHeader, FrameReader, FrameView, MAX_HEADER_SIZE,
    OptimizationProfile, VectoredFrame, pack_frame, pack_frame_vectored, unpack_frame,
};
pub use record::{CbpRecord, FieldCodec, StitchRecord};
pub use resync::{ResyncStats, StreamResyncParser, find_magic, find_magic_swar, resync_frame};
pub use varint::{decode_prefix_varint, encode_prefix_varint};
pub use zigzag::{decode_zigzag32, decode_zigzag64, encode_zigzag32, encode_zigzag64};
