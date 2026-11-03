//! High-density data layout, zero-allocation transformation engine and columnar codecs (Stitch Data Engine).

#![cfg_attr(not(feature = "std"), no_std)]

pub use stitch_data_core;
pub use stitch_data_transforms;

pub use stitch_data_core::{
    CBP_MAGIC, CBP_VERSION_1, ColumnCodec, CompatibilityResult, FieldCodec, FieldDescriptor,
    FrameFlags, FrameHeader, FrameReader, FrameView, MAX_HEADER_SIZE, OptimizationProfile, Record,
    ResyncStats, SchemaCompatibilityVerifier, SchemaDefinition, StitchError, StreamResyncParser,
    VectoredFrame, bitstream, crc, error, frame, record, resync, schema, simd, varint, zigzag,
};

pub use stitch_data_transforms::{
    BitPacker, ColumnCodecTag, ColumnSlice, ColumnSliceIndex, ColumnarBlockBuilder,
    ColumnarBlockReader, ColumnarBlockWriter, CompressionPipeline, DecompressionPipeline,
    DeltaTransformer, DictTransformer, GorillaCompressor, RecordBatchBuilder, RleTransformer,
    bitpack, columnar, decode_columnar_frame, decode_dict_column, decode_f32_column,
    decode_i32_column, decode_raw_column, decode_rle_column, decode_u32_column, delta, dict,
    encode_dict_column, encode_f32_column, encode_i32_column, encode_raw_column, encode_rle_column,
    encode_u32_column, float, pipeline, read_bitpacked_u32_at, rle,
};

#[cfg(feature = "derive")]
pub use stitch_data_derive::Record;

/// Convenient prelude module re-exporting key framing, types, and transform pipelines.
pub mod prelude {
    pub use stitch_data_core::bitstream::{BitReader, BitWriter};
    pub use stitch_data_core::crc::{
        crc32_castagnoli, crc32_castagnoli_finalize, crc32_castagnoli_init,
        crc32_castagnoli_slices, crc32_castagnoli_update,
    };
    pub use stitch_data_core::error::StitchError;
    pub use stitch_data_core::frame::{
        CBP_MAGIC, CBP_VERSION_1, FrameFlags, FrameHeader, FrameReader, FrameView, MAX_HEADER_SIZE,
        OptimizationProfile, VectoredFrame, pack_frame, pack_frame_vectored, unpack_frame,
    };
    pub use stitch_data_core::record::{ColumnCodec, FieldCodec, Record};
    pub use stitch_data_core::resync::{
        ResyncStats, StreamResyncParser, find_magic, find_magic_swar, resync_frame,
    };
    pub use stitch_data_core::varint::{decode_prefix_varint, encode_prefix_varint};
    pub use stitch_data_core::zigzag::{
        decode_zigzag32, decode_zigzag64, encode_zigzag32, encode_zigzag64,
    };

    pub use stitch_data_transforms::bitpack::BitPacker;
    pub use stitch_data_transforms::columnar::{
        ColumnCodecTag, ColumnSlice, ColumnSliceIndex, ColumnarBlockBuilder, ColumnarBlockReader,
        ColumnarBlockWriter, RecordBatchBuilder, decode_columnar_frame, decode_dict_column,
        decode_f32_column, decode_i32_column, decode_raw_column, decode_rle_column,
        decode_u32_column, encode_dict_column, encode_f32_column, encode_i32_column,
        encode_raw_column, encode_rle_column, encode_u32_column, read_bitpacked_u32_at,
    };
    pub use stitch_data_transforms::delta::{
        DeltaTransformer, delta_decode_i32, delta_decode_u32, delta_decode_with_base_u32,
        delta_encode_i32, delta_encode_u32, delta_encode_with_base_u32,
    };
    pub use stitch_data_transforms::dict::DictTransformer;
    pub use stitch_data_transforms::float::GorillaCompressor;
    pub use stitch_data_transforms::pipeline::{CompressionPipeline, DecompressionPipeline};
    pub use stitch_data_transforms::rle::RleTransformer;

    #[cfg(feature = "derive")]
    pub use stitch_data_derive::Record;
}
