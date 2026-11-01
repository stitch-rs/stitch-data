//! Core `#![no_std]` framing, bitstream, PrefixVarint, and ZigZag primitives.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bitstream;
pub mod crc;
pub mod error;
pub mod frame;
pub mod varint;
pub mod zigzag;

pub use error::CbpError;
pub use frame::{CBP_MAGIC, CBP_VERSION_1, FrameFlags, FrameHeader, OptimizationProfile};
pub use varint::{decode_prefix_varint, encode_prefix_varint};
pub use zigzag::{decode_zigzag32, decode_zigzag64, encode_zigzag32, encode_zigzag64};
