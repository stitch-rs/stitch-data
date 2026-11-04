//! Autonomous Data Profiling and Heuristic Analysis Engine.
//!
//! Evaluates monotonicity, cardinality, run-length frequency, and entropy
//! to recommend optimal zero-waste codecs (Delta, BitPacking, RLE, Dict, Gorilla).

use stitch_data::ColumnCodecTag;

/// Profiling summary and recommendations for a numeric or byte sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfileReport {
    pub total_count: usize,
    pub unique_count: usize,
    pub is_monotonic: bool,
    pub avg_run_length: f64,
    pub max_value: u64,
    pub min_value: u64,
    pub recommended_codec: ColumnCodecTag,
    pub bit_width: u8,
    pub reason: String,
}

pub struct DataProfiler;

impl DataProfiler {
    /// Analyzes a slice of unsigned 32-bit integers.
    pub fn profile_u32(values: &[u32]) -> ColumnProfileReport {
        if values.is_empty() {
            return ColumnProfileReport {
                total_count: 0,
                unique_count: 0,
                is_monotonic: true,
                avg_run_length: 0.0,
                max_value: 0,
                min_value: 0,
                recommended_codec: ColumnCodecTag::Raw,
                bit_width: 32,
                reason: "Empty column".to_string(),
            };
        }

        let total = values.len();
        let mut min_val = values[0];
        let mut max_val = values[0];
        let mut is_monotonic = true;

        let mut run_count = 1;
        let mut unique_set = std::collections::HashSet::new();

        for i in 0..total {
            let v = values[i];
            if v < min_val {
                min_val = v;
            }
            if v > max_val {
                max_val = v;
            }
            if i > 0 {
                if v < values[i - 1] {
                    is_monotonic = false;
                }
                if v != values[i - 1] {
                    run_count += 1;
                }
            }
            if unique_set.len() < 1024 {
                unique_set.insert(v);
            }
        }

        let avg_run_length = total as f64 / run_count as f64;
        let unique_ratio = unique_set.len() as f64 / total as f64;
        let bit_width = if max_val == 0 {
            1
        } else {
            32 - max_val.leading_zeros() as u8
        };

        // Decision logic
        if avg_run_length >= 3.0 {
            ColumnProfileReport {
                total_count: total,
                unique_count: unique_set.len(),
                is_monotonic,
                avg_run_length,
                max_value: max_val as u64,
                min_value: min_val as u64,
                recommended_codec: ColumnCodecTag::Rle,
                bit_width,
                reason: format!(
                    "High run repetition detected (avg run: {avg_run_length:.2}) -> RLE"
                ),
            }
        } else if unique_set.len() <= 256 && unique_ratio < 0.15 {
            let pct = unique_ratio * 100.0;
            ColumnProfileReport {
                total_count: total,
                unique_count: unique_set.len(),
                is_monotonic,
                avg_run_length,
                max_value: max_val as u64,
                min_value: min_val as u64,
                recommended_codec: ColumnCodecTag::Dictionary,
                bit_width: (unique_set.len() as f64).log2().ceil() as u8,
                reason: format!(
                    "Low cardinality ({} unique values, ratio {pct:.1}%) -> Dictionary",
                    unique_set.len()
                ),
            }
        } else if is_monotonic {
            ColumnProfileReport {
                total_count: total,
                unique_count: unique_set.len(),
                is_monotonic,
                avg_run_length,
                max_value: max_val as u64,
                min_value: min_val as u64,
                recommended_codec: ColumnCodecTag::AdaptiveU32,
                bit_width,
                reason: "Monotonically increasing sequence -> Delta + BitPacking".to_string(),
            }
        } else {
            ColumnProfileReport {
                total_count: total,
                unique_count: unique_set.len(),
                is_monotonic,
                avg_run_length,
                max_value: max_val as u64,
                min_value: min_val as u64,
                recommended_codec: ColumnCodecTag::AdaptiveU32,
                bit_width,
                reason: format!("Arbitrary integers fit in {bit_width} bits -> BitPacking"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_monotonic_recommends_delta_bitpack() {
        let monotonic: Vec<u32> = (1_000..2_000).collect();
        let report = DataProfiler::profile_u32(&monotonic);
        assert_eq!(report.recommended_codec, ColumnCodecTag::AdaptiveU32);
        assert!(report.is_monotonic);
    }

    #[test]
    fn test_profile_runs_recommends_rle() {
        let mut rle_data = Vec::new();
        for _ in 0..10 {
            rle_data.extend(vec![42; 50]);
            rle_data.extend(vec![99; 50]);
        }
        let report = DataProfiler::profile_u32(&rle_data);
        assert_eq!(report.recommended_codec, ColumnCodecTag::Rle);
        assert!(report.avg_run_length >= 25.0);
    }

    #[test]
    fn test_profile_categorical_recommends_dictionary() {
        let mut dict_data = Vec::new();
        for i in 0..1000 {
            dict_data.push((i % 5) as u32);
        }
        let report = DataProfiler::profile_u32(&dict_data);
        assert_eq!(report.recommended_codec, ColumnCodecTag::Dictionary);
        assert_eq!(report.unique_count, 5);
    }
}
