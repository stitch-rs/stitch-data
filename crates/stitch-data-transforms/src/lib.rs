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
pub use columnar::{ColumnarBlockBuilder, RecordBatchBuilder};
pub use delta::DeltaTransformer;
pub use float::GorillaCompressor;
pub use pipeline::{CompressionPipeline, DecompressionPipeline};
pub use rle::RleTransformer;
