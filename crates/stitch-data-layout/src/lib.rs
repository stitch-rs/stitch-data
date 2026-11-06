//! Zero-cost cache-line fitting, alignment geometry, and super-block layout manager.
//!
//! Designed according to the "Scrooge" Systems Mindset:
//! - Guarantees zero cache-line straddling across hot lookup paths.
//! - Pure `#![no_std]` core with zero dynamic memory overhead.
//! - Compile-time `const fn` layout and tile geometry solvers.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod geometry;
pub mod superblock;

pub use geometry::{CacheGeometry, DefaultCacheLine, LINE_SIZE_64, LINE_SIZE_128};
pub use superblock::{CacheAligned, CachePadded, CacheSuperBlock};
