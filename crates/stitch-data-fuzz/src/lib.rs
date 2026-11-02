//! Adversarial fuzzing target harnesses for Compact Binary Protocol (CBP).
//!
//! Provides libFuzzer and deterministic fuzz testing entrypoints across all
//! protocol decoders and transformation kernels. Every function in this module
//! is strictly required to NEVER panic or invoke undefined behavior on arbitrary
//! untrusted byte slices.

use stitch_data::prelude::*;

/// Target model for record-level fuzzing.
#[derive(StitchRecord, Debug, PartialEq, Clone, Copy, Default)]
pub struct FuzzRecord {
    #[stitch(varint)]
    pub id: u32,
    #[stitch(zigzag)]
    pub delta_x: i32,
    #[stitch(raw)]
    pub coord_y: f32,
    #[stitch(bits = 6)]
    pub tag: u8,
    pub is_valid: bool,
}

/// Fuzzes `stitch_data_core::frame::unpack_frame` with arbitrary untrusted bytes.
///
/// Guaranteed to return `Ok` or `Err(CbpError)`, never panic.
#[inline]
pub fn fuzz_unpack_frame(data: &[u8]) {
    let _ = unpack_frame(data);
}

/// Fuzzes `stitch_data_core::frame::FrameReader` streaming decoder with arbitrary bytes.
#[inline]
pub fn fuzz_frame_reader(data: &[u8]) {
    let mut reader = FrameReader::new(data);
    while let Ok(Some(_frame)) = reader.next_frame() {}
}

/// Fuzzes `stitch_data_core::varint::decode_prefix_varint` with arbitrary bytes.
#[inline]
pub fn fuzz_prefix_varint(data: &[u8]) {
    let _ = decode_prefix_varint(data);
}

/// Fuzzes `stitch_data_transforms::bitpack::BitPacker::unpack_u32` with arbitrary bit-widths.
#[inline]
pub fn fuzz_bitpack(data: &[u8]) {
    let mut dst = [0u32; 64];
    let _ = BitPacker::unpack_u32(data, &mut dst);
}

/// Fuzzes `stitch_data_core::resync::StreamResyncParser` with arbitrary streaming chunks.
#[inline]
pub fn fuzz_stream_resync(data: &[u8]) {
    let mut parser = StreamResyncParser::new();
    let mut offset = 0;
    while offset < data.len() {
        if let Some((consumed, _frame)) = parser.parse_next(&data[offset..]) {
            offset += consumed.max(1);
        } else {
            break;
        }
    }
}

/// Fuzzes `stitch_data_transforms::float::GorillaCompressor` float decompressors.
#[inline]
pub fn fuzz_gorilla_float(data: &[u8]) {
    let mut dst_f32 = [0.0f32; 32];
    let _ = GorillaCompressor::decompress_f32(data, &mut dst_f32);

    let mut dst_f64 = [0.0f64; 32];
    let _ = GorillaCompressor::decompress_f64(data, &mut dst_f64);
}

/// Fuzzes `StitchRecord` generated deserializers with arbitrary bytes.
#[inline]
pub fn fuzz_derived_record(data: &[u8]) {
    let _ = FuzzRecord::decode_record(data);
    let _ = FuzzRecord::unpack_from_frame(data);
}
