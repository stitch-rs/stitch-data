//! Compact Binary Protocol (CBP) high-performance facade.

#![cfg_attr(not(feature = "std"), no_std)]

pub use cbp_core::*;
pub use cbp_transforms::*;

/// Convenient prelude module re-exporting key framing, types, and transform pipelines.
pub mod prelude {
    pub use cbp_core::bitstream::{BitReader, BitWriter};
    pub use cbp_core::crc::crc32_castagnoli;
    pub use cbp_core::error::CbpError;
    pub use cbp_core::frame::{
        CBP_MAGIC, CBP_VERSION_1, FrameFlags, FrameHeader, OptimizationProfile, pack_frame,
        unpack_frame,
    };
    pub use cbp_core::varint::{decode_prefix_varint, encode_prefix_varint};
    pub use cbp_core::zigzag::{
        decode_zigzag32, decode_zigzag64, encode_zigzag32, encode_zigzag64,
    };

    pub use cbp_transforms::bitpack::BitPacker;
    pub use cbp_transforms::delta::{
        DeltaTransformer, delta_decode_u32, delta_decode_with_base_u32, delta_encode_u32,
        delta_encode_with_base_u32,
    };
    pub use cbp_transforms::pipeline::{CompressionPipeline, DecompressionPipeline};
    pub use cbp_transforms::rle::RleTransformer;
}
