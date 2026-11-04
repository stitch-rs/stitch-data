//! What-If Codec Simulator for Stitch Data columns.
//!
//! Evaluates compression size and round-trip decompression speed across candidate codecs:
//! - Raw (Uncompressed POD)
//! - Delta + BitPacking
//! - Direct BitPacking
//! - Run-Length Encoding (RLE)
//! - Dictionary Encoding

use std::time::Instant;
use stitch_data::ColumnCodecTag;
use stitch_data::columnar::{
    decode_dict_column, decode_raw_column, decode_rle_column, decode_u32_column,
    encode_dict_column, encode_raw_column, encode_rle_column, encode_u32_column,
};

/// Performance and footprint metrics of a candidate codec on a test column.
#[derive(Debug, Clone, PartialEq)]
pub struct CodecSimulationResult {
    pub codec: ColumnCodecTag,
    pub name: &'static str,
    pub compressed_bytes: usize,
    pub compression_ratio: f64,
    pub encode_time_micros: f64,
    pub decode_time_micros: f64,
}

pub struct CodecSimulator;

impl CodecSimulator {
    /// Simulates and benchmarks all available codecs on a slice of u32 values.
    pub fn simulate_u32(values: &[u32]) -> Vec<CodecSimulationResult> {
        let mut results = Vec::new();
        let total = values.len();
        if total == 0 {
            return results;
        }

        let raw_size = total * 4;

        // 1. Raw POD
        let raw_bytes_slice: &[u8] =
            unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, raw_size) };
        let mut raw_buf = vec![0u8; raw_size];
        let t0 = Instant::now();
        let raw_len = encode_raw_column(raw_bytes_slice, &mut raw_buf).unwrap_or(0);
        let raw_enc_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        let mut raw_dec = vec![0u32; total];
        let raw_dec_bytes: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(raw_dec.as_mut_ptr() as *mut u8, raw_size) };
        let t0 = Instant::now();
        let _ = decode_raw_column(&raw_buf[..raw_len], raw_dec_bytes);
        let raw_dec_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        results.push(CodecSimulationResult {
            codec: ColumnCodecTag::Raw,
            name: "Raw (POD)",
            compressed_bytes: raw_len,
            compression_ratio: raw_len as f64 / raw_size as f64,
            encode_time_micros: raw_enc_us,
            decode_time_micros: raw_dec_us,
        });

        // 2. Adaptive U32 (Delta + BitPacking / Direct BitPacking)
        let mut scratch = vec![0u32; total];
        let mut col_buf = vec![0u8; total * 5 + 64];
        let t0 = Instant::now();
        let col_len = encode_u32_column(values, &mut scratch, &mut col_buf).unwrap_or(0);
        let col_enc_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        let mut col_dec = vec![0u32; total];
        let t0 = Instant::now();
        let _ = decode_u32_column(&col_buf[..col_len], &mut col_dec);
        let col_dec_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        results.push(CodecSimulationResult {
            codec: ColumnCodecTag::AdaptiveU32,
            name: "Adaptive U32 (Delta/BitPack)",
            compressed_bytes: col_len,
            compression_ratio: col_len as f64 / raw_size as f64,
            encode_time_micros: col_enc_us,
            decode_time_micros: col_dec_us,
        });

        // 3. RLE
        let mut rle_buf = vec![0u8; total * 6 + 64];
        let t0 = Instant::now();
        let rle_res = encode_rle_column(values, &mut rle_buf);
        let rle_enc_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        if let Ok(rle_len) = rle_res {
            let mut rle_dec = vec![0u32; total];
            let t0 = Instant::now();
            let _ = decode_rle_column(&rle_buf[..rle_len], &mut rle_dec);
            let rle_dec_us = t0.elapsed().as_nanos() as f64 / 1000.0;

            results.push(CodecSimulationResult {
                codec: ColumnCodecTag::Rle,
                name: "RLE (Run-Length)",
                compressed_bytes: rle_len,
                compression_ratio: rle_len as f64 / raw_size as f64,
                encode_time_micros: rle_enc_us,
                decode_time_micros: rle_dec_us,
            });
        }

        // 4. Dictionary
        let mut dict_buf = vec![0u8; total * 4 + 1024];
        let t0 = Instant::now();
        let dict_res = encode_dict_column(values, &mut dict_buf);
        let dict_enc_us = t0.elapsed().as_nanos() as f64 / 1000.0;

        if let Ok(dict_len) = dict_res {
            let mut dict_dec = vec![0u32; total];
            let t0 = Instant::now();
            let _ = decode_dict_column(&dict_buf[..dict_len], &mut dict_dec);
            let dict_dec_us = t0.elapsed().as_nanos() as f64 / 1000.0;

            results.push(CodecSimulationResult {
                codec: ColumnCodecTag::Dictionary,
                name: "Dictionary",
                compressed_bytes: dict_len,
                compression_ratio: dict_len as f64 / raw_size as f64,
                encode_time_micros: dict_enc_us,
                decode_time_micros: dict_dec_us,
            });
        }

        // Sort results by compressed bytes ascending (most compact first)
        results.sort_by_key(|r| r.compressed_bytes);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulator_monotonic_finds_bitpack_best() {
        let vals: Vec<u32> = (100_000..101_000).collect();
        let sim = CodecSimulator::simulate_u32(&vals);
        assert!(!sim.is_empty());
        // The most compact codec should be AdaptiveU32 (Delta+Bitpack)
        assert_eq!(sim[0].codec, ColumnCodecTag::AdaptiveU32);
        assert!(sim[0].compressed_bytes < 4_000); // Raw is 4000 bytes
    }

    #[test]
    fn test_simulator_runs_finds_rle_best() {
        let mut vals = Vec::new();
        for i in 0..10 {
            vals.extend(vec![i as u32 * 100; 100]);
        }
        let sim = CodecSimulator::simulate_u32(&vals);
        assert_eq!(sim[0].codec, ColumnCodecTag::Rle);
        assert!(sim[0].compressed_bytes < 500); // ~10 runs * 5 bytes = ~50 bytes
    }
}
