//! Monomorphic transformation pipelines powered by `stitch-core`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bitpack;
pub mod columnar;
pub mod delta;
pub mod float;
pub mod pipeline;
pub mod rle;
pub mod simd;

pub use bitpack::BitPacker;
pub use columnar::{
    ColumnCodecTag, ColumnarBlockBuilder, ColumnarBlockReader, ColumnarBlockWriter,
    RecordBatchBuilder, decode_columnar_frame, decode_f32_column, decode_i32_column,
    decode_raw_column, decode_u32_column, encode_f32_column, encode_i32_column, encode_raw_column,
    encode_u32_column,
};
pub use delta::DeltaTransformer;
pub use float::GorillaCompressor;
pub use pipeline::{CompressionPipeline, DecompressionPipeline};
pub use rle::RleTransformer;
