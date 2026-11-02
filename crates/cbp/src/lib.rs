//! Compact Binary Protocol (CBP) high-performance facade.

#![cfg_attr(not(feature = "std"), no_std)]

pub use cbp_core;
pub use cbp_transforms;

pub use cbp_core::{
    CBP_MAGIC, CBP_VERSION_1, CbpError, CbpRecord, FieldCodec, FrameFlags, FrameHeader,
    FrameReader, FrameView, MAX_HEADER_SIZE, OptimizationProfile, ResyncStats, StreamResyncParser,
    VectoredFrame, bitstream, crc, error, frame, record, resync, simd, varint, zigzag,
};

pub use cbp_transforms::{
    BitPacker, ColumnarBlockBuilder, CompressionPipeline, DecompressionPipeline, DeltaTransformer,
    GorillaCompressor, RecordBatchBuilder, RleTransformer, bitpack, columnar, delta, float,
    pipeline, rle,
};

#[cfg(feature = "derive")]
pub use cbp_derive::CbpRecord;

/// Convenient prelude module re-exporting key framing, types, and transform pipelines.
pub mod prelude {
    pub use cbp_core::bitstream::{BitReader, BitWriter};
    pub use cbp_core::crc::{
        crc32_castagnoli, crc32_castagnoli_finalize, crc32_castagnoli_init,
        crc32_castagnoli_slices, crc32_castagnoli_update,
    };
    pub use cbp_core::error::CbpError;
    pub use cbp_core::frame::{
        CBP_MAGIC, CBP_VERSION_1, FrameFlags, FrameHeader, FrameReader, FrameView, MAX_HEADER_SIZE,
        OptimizationProfile, VectoredFrame, pack_frame, pack_frame_vectored, unpack_frame,
    };
    pub use cbp_core::record::{CbpRecord, FieldCodec};
    pub use cbp_core::resync::{
        ResyncStats, StreamResyncParser, find_magic, find_magic_swar, resync_frame,
    };
    pub use cbp_core::varint::{decode_prefix_varint, encode_prefix_varint};
    pub use cbp_core::zigzag::{
        decode_zigzag32, decode_zigzag64, encode_zigzag32, encode_zigzag64,
    };

    pub use cbp_transforms::bitpack::BitPacker;
    pub use cbp_transforms::columnar::{ColumnarBlockBuilder, RecordBatchBuilder};
    pub use cbp_transforms::delta::{
        DeltaTransformer, delta_decode_i32, delta_decode_u32, delta_decode_with_base_u32,
        delta_encode_i32, delta_encode_u32, delta_encode_with_base_u32,
    };
    pub use cbp_transforms::float::GorillaCompressor;
    pub use cbp_transforms::pipeline::{CompressionPipeline, DecompressionPipeline};
    pub use cbp_transforms::rle::RleTransformer;

    #[cfg(feature = "derive")]
    pub use cbp_derive::CbpRecord;
}
