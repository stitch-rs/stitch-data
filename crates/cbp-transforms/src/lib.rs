//! Monomorphic transformation pipelines powered by `stitch-core`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bitpack;
pub mod delta;
pub mod rle;

pub use bitpack::BitPacker;
pub use delta::DeltaTransformer;
pub use rle::RleTransformer;
