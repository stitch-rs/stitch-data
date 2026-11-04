//! Autonomous Multi-Column Dataset Profiler & Codec Auto-Tuner.
//!
//! Provides statistical analysis, Shannon entropy calculation, monotonicity tracking,
//! and schema inference for numeric, categorical, and float columns.

use std::collections::{HashMap, HashSet};
use stitch_data::ColumnCodecTag;

/// Inferred datatype for a column during statistical parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferredDataType {
    U32,
    I32,
    F32,
    CategoryString,
    BinaryFlags,
}

/// Detailed profiling report for a single dataset column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfileReport {
    pub name: String,
    pub index: usize,
    pub total_count: usize,
    pub unique_count: usize,
    pub data_type: InferredDataType,
    pub is_monotonic: bool,
    pub avg_run_length: f64,
    pub shannon_entropy: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub bit_width: u8,
    pub recommended_codec: ColumnCodecTag,
    pub estimated_compressed_bytes: usize,
    pub raw_bytes: usize,
    pub reason: String,
}

/// Complete dataset schema recommendation and profiling summary.
#[derive(Debug, Clone, PartialEq)]
pub struct DatasetProfileReport {
    pub total_rows: usize,
    pub total_columns: usize,
    pub columns: Vec<ColumnProfileReport>,
    pub total_raw_bytes: usize,
    pub total_estimated_compressed_bytes: usize,
}

impl DatasetProfileReport {
    /// Overall compression ratio achieved (e.g. 0.20 = 80% reduction).
    pub fn compression_ratio(&self) -> f64 {
        if self.total_raw_bytes == 0 {
            1.0
        } else {
            self.total_estimated_compressed_bytes as f64 / self.total_raw_bytes as f64
        }
    }

    /// Generates a declarative TOML schema configuration for the analyzed dataset.
    pub fn to_toml_config(&self) -> String {
        let mut out = String::new();
        out.push_str("# Auto-generated Stitch Data Compression Preset\n");
        out.push_str("[dataset]\n");
        out.push_str(&format!("rows = {}\n", self.total_rows));
        out.push_str(&format!("columns = {}\n", self.total_columns));
        out.push_str(&format!(
            "estimated_space_saving_pct = {:.1}\n\n",
            (1.0 - self.compression_ratio()) * 100.0
        ));

        for col in &self.columns {
            out.push_str(&format!("[[columns]]\nname = \"{}\"\n", col.name));
            out.push_str(&format!("index = {}\n", col.index));
            out.push_str(&format!("type = \"{:?}\"\n", col.data_type));
            out.push_str(&format!("codec = \"{:?}\"\n", col.recommended_codec));
            out.push_str(&format!("bit_width = {}\n", col.bit_width));
            out.push_str(&format!("reason = \"{}\"\n\n", col.reason));
        }
        out
    }

    /// Generates a Rust struct definition with Stitch Data field annotations.
    pub fn to_rust_struct(&self, struct_name: &str) -> String {
        let mut out = String::new();
        out.push_str("use stitch_data::prelude::*;\n\n");
        out.push_str("#[derive(Debug, Clone, PartialEq, Record)]\n");
        out.push_str(&format!("pub struct {struct_name} {{\n"));

        for col in &self.columns {
            let field_ident = sanitize_identifier(&col.name);
            let rust_type = match col.data_type {
                InferredDataType::U32 => "u32",
                InferredDataType::I32 => "i32",
                InferredDataType::F32 => "f32",
                InferredDataType::CategoryString => "String",
                InferredDataType::BinaryFlags => "u32",
            };

            let codec_attr = match col.recommended_codec {
                ColumnCodecTag::AdaptiveU32 => {
                    if col.is_monotonic {
                        "#[column(codec = \"delta_bitpack\")]"
                    } else {
                        "#[column(codec = \"bitpack\")]"
                    }
                }
                ColumnCodecTag::Rle => "#[column(codec = \"rle\")]",
                ColumnCodecTag::Dictionary => "#[column(codec = \"dict\")]",
                ColumnCodecTag::GorillaF32 => "#[column(codec = \"gorilla\")]",
                _ => "#[column(codec = \"raw\")]",
            };

            out.push_str(&format!("    {codec_attr}\n"));
            out.push_str(&format!("    pub {field_ident}: {rust_type},\n"));
        }
        out.push_str("}\n");
        out
    }
}

pub struct DataProfiler;

impl DataProfiler {
    /// Profiles a single slice of u32 integers.
    pub fn profile_u32(name: &str, index: usize, values: &[u32]) -> ColumnProfileReport {
        if values.is_empty() {
            return ColumnProfileReport {
                name: name.to_string(),
                index,
                total_count: 0,
                unique_count: 0,
                data_type: InferredDataType::U32,
                is_monotonic: true,
                avg_run_length: 0.0,
                shannon_entropy: 0.0,
                min_value: 0.0,
                max_value: 0.0,
                bit_width: 32,
                recommended_codec: ColumnCodecTag::Raw,
                estimated_compressed_bytes: 0,
                raw_bytes: 0,
                reason: "Empty column".to_string(),
            };
        }

        let total = values.len();
        let raw_bytes = total * 4;
        let mut min_val = values[0];
        let mut max_val = values[0];
        let mut is_monotonic = true;
        let mut run_count = 1;

        let mut freq_map = HashMap::new();
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
            if freq_map.len() < 4096 {
                *freq_map.entry(v).or_insert(0usize) += 1;
            }
        }

        let avg_run_length = total as f64 / run_count as f64;
        let unique_count = freq_map.len();
        let unique_ratio = unique_count as f64 / total as f64;

        // Calculate Shannon Entropy: H = - sum(p * log2(p))
        let mut entropy = 0.0f64;
        for &count in freq_map.values() {
            let p = count as f64 / total as f64;
            if p > 0.0 {
                entropy -= p * p.log2();
            }
        }

        // Direct BitWidth
        let bit_width = if max_val == 0 {
            1
        } else {
            32 - max_val.leading_zeros() as u8
        };

        // Decision logic & byte estimation
        if avg_run_length >= 3.0 {
            let estimated = run_count * 5; // ~4 bytes value + 1 byte run length
            ColumnProfileReport {
                name: name.to_string(),
                index,
                total_count: total,
                unique_count,
                data_type: InferredDataType::U32,
                is_monotonic,
                avg_run_length,
                shannon_entropy: entropy,
                min_value: min_val as f64,
                max_value: max_val as f64,
                bit_width,
                recommended_codec: ColumnCodecTag::Rle,
                estimated_compressed_bytes: estimated.min(raw_bytes),
                raw_bytes,
                reason: format!("High run repetition (avg run: {avg_run_length:.2}) -> RLE"),
            }
        } else if unique_count <= 256 && unique_ratio < 0.15 {
            let dict_bits = (unique_count as f64).log2().ceil().max(1.0) as usize;
            let indices_bytes = (total * dict_bits).div_ceil(8);
            let table_bytes = unique_count * 4;
            let estimated = indices_bytes + table_bytes;
            ColumnProfileReport {
                name: name.to_string(),
                index,
                total_count: total,
                unique_count,
                data_type: InferredDataType::U32,
                is_monotonic,
                avg_run_length,
                shannon_entropy: entropy,
                min_value: min_val as f64,
                max_value: max_val as f64,
                bit_width: dict_bits as u8,
                recommended_codec: ColumnCodecTag::Dictionary,
                estimated_compressed_bytes: estimated.min(raw_bytes),
                raw_bytes,
                reason: format!(
                    "Low cardinality ({unique_count} distinct symbols, H={entropy:.2}b) -> Dictionary"
                ),
            }
        } else if is_monotonic {
            // Delta max value
            let mut max_delta = 0u32;
            for i in 1..total {
                let d = values[i].saturating_sub(values[i - 1]);
                if d > max_delta {
                    max_delta = d;
                }
            }
            let delta_bits = if max_delta == 0 {
                1
            } else {
                32 - max_delta.leading_zeros() as u8
            };
            let estimated = 4 + (total * delta_bits as usize).div_ceil(8); // base 4B + bitpacked deltas
            ColumnProfileReport {
                name: name.to_string(),
                index,
                total_count: total,
                unique_count,
                data_type: InferredDataType::U32,
                is_monotonic: true,
                avg_run_length,
                shannon_entropy: entropy,
                min_value: min_val as f64,
                max_value: max_val as f64,
                bit_width: delta_bits,
                recommended_codec: ColumnCodecTag::AdaptiveU32,
                estimated_compressed_bytes: estimated.min(raw_bytes),
                raw_bytes,
                reason: format!(
                    "Monotonic sequence (max delta requires {delta_bits} bits) -> Delta + BitPacking"
                ),
            }
        } else {
            let estimated = (total * bit_width as usize).div_ceil(8);
            ColumnProfileReport {
                name: name.to_string(),
                index,
                total_count: total,
                unique_count,
                data_type: InferredDataType::U32,
                is_monotonic,
                avg_run_length,
                shannon_entropy: entropy,
                min_value: min_val as f64,
                max_value: max_val as f64,
                bit_width,
                recommended_codec: ColumnCodecTag::AdaptiveU32,
                estimated_compressed_bytes: estimated.min(raw_bytes),
                raw_bytes,
                reason: format!(
                    "Bounded non-monotonic integers fit in {bit_width} bits -> Direct BitPacking"
                ),
            }
        }
    }

    /// Profiles a slice of 32-bit floats.
    pub fn profile_f32(name: &str, index: usize, values: &[f32]) -> ColumnProfileReport {
        let total = values.len();
        let raw_bytes = total * 4;
        let mut min_val = f32::MAX;
        let mut max_val = f32::MIN;

        for &v in values {
            if v < min_val {
                min_val = v;
            }
            if v > max_val {
                max_val = v;
            }
        }

        // Gorilla compression typically yields ~50% reduction on floating point telemetry/coords
        let estimated = (raw_bytes as f64 * 0.55) as usize;
        ColumnProfileReport {
            name: name.to_string(),
            index,
            total_count: total,
            unique_count: total,
            data_type: InferredDataType::F32,
            is_monotonic: false,
            avg_run_length: 1.0,
            shannon_entropy: 0.0,
            min_value: min_val as f64,
            max_value: max_val as f64,
            bit_width: 32,
            recommended_codec: ColumnCodecTag::GorillaF32,
            estimated_compressed_bytes: estimated,
            raw_bytes,
            reason: "Floating-point coordinates/telemetry sequence -> Gorilla XOR".to_string(),
        }
    }

    /// Profiles string categorical values.
    pub fn profile_strings(name: &str, index: usize, values: &[&str]) -> ColumnProfileReport {
        let total = values.len();
        let raw_bytes: usize = values.iter().map(|s| s.len() + 1).sum();
        let mut unique_set = HashSet::new();

        for &s in values {
            if unique_set.len() < 1024 {
                unique_set.insert(s);
            }
        }

        let unique_count = unique_set.len();
        let dict_bits = (unique_count as f64).log2().ceil().max(1.0) as usize;
        let table_bytes: usize = unique_set.iter().map(|s| s.len() + 1).sum();
        let indices_bytes = (total * dict_bits).div_ceil(8);
        let estimated = table_bytes + indices_bytes;

        ColumnProfileReport {
            name: name.to_string(),
            index,
            total_count: total,
            unique_count,
            data_type: InferredDataType::CategoryString,
            is_monotonic: false,
            avg_run_length: 1.0,
            shannon_entropy: 0.0,
            min_value: 0.0,
            max_value: unique_count as f64,
            bit_width: dict_bits as u8,
            recommended_codec: ColumnCodecTag::Dictionary,
            estimated_compressed_bytes: estimated.min(raw_bytes),
            raw_bytes,
            reason: format!(
                "Text categories ({unique_count} distinct symbols) -> Deduplicated Dictionary"
            ),
        }
    }

    /// Parses CSV/TSV contents, infers column schemas, and profiles all columns concurrently.
    pub fn profile_csv(
        content: &str,
        delimiter: char,
        has_header: bool,
    ) -> Result<DatasetProfileReport, String> {
        let mut lines = content.lines().filter(|l| !l.trim().is_empty());
        let first_line = match lines.next() {
            Some(l) => l,
            None => return Err("CSV input is empty".to_string()),
        };

        let raw_headers: Vec<String> = if has_header {
            first_line
                .split(delimiter)
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            let cols = first_line.split(delimiter).count();
            (0..cols).map(|i| format!("col_{i}")).collect()
        };

        let mut row_records: Vec<Vec<String>> = Vec::new();
        if !has_header {
            row_records.push(
                first_line
                    .split(delimiter)
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }

        for line in lines {
            let row: Vec<String> = line
                .split(delimiter)
                .map(|s| s.trim().to_string())
                .collect();
            if row.len() == raw_headers.len() {
                row_records.push(row);
            }
        }

        let total_rows = row_records.len();
        if total_rows == 0 {
            return Err("No data rows found in CSV".to_string());
        }

        let mut columns = Vec::new();
        let mut total_raw_bytes = 0;
        let mut total_compressed_bytes = 0;

        for col_idx in 0..raw_headers.len() {
            let col_name = &raw_headers[col_idx];
            let sample_cells: Vec<&str> = row_records.iter().map(|r| r[col_idx].as_str()).collect();

            // Detect type: try u32, then f32, then string
            let all_u32 = sample_cells.iter().all(|s| s.parse::<u32>().is_ok());
            let report = if all_u32 {
                let u32_vals: Vec<u32> = sample_cells
                    .iter()
                    .map(|s| s.parse::<u32>().unwrap())
                    .collect();
                Self::profile_u32(col_name, col_idx, &u32_vals)
            } else {
                let all_f32 = sample_cells.iter().all(|s| s.parse::<f32>().is_ok());
                if all_f32 {
                    let f32_vals: Vec<f32> = sample_cells
                        .iter()
                        .map(|s| s.parse::<f32>().unwrap())
                        .collect();
                    Self::profile_f32(col_name, col_idx, &f32_vals)
                } else {
                    Self::profile_strings(col_name, col_idx, &sample_cells)
                }
            };

            total_raw_bytes += report.raw_bytes;
            total_compressed_bytes += report.estimated_compressed_bytes;
            columns.push(report);
        }

        Ok(DatasetProfileReport {
            total_rows,
            total_columns: columns.len(),
            columns,
            total_raw_bytes,
            total_estimated_compressed_bytes: total_compressed_bytes,
        })
    }
}

fn sanitize_identifier(name: &str) -> String {
    let mut clean = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
            clean.push(c.to_ascii_lowercase());
        } else {
            clean.push('_');
        }
    }
    if clean.is_empty() || clean.chars().next().unwrap().is_numeric() {
        format!("field_{clean}")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_monotonic_recommends_delta_bitpack() {
        let monotonic: Vec<u32> = (1_000..2_000).collect();
        let report = DataProfiler::profile_u32("id", 0, &monotonic);
        assert_eq!(report.recommended_codec, ColumnCodecTag::AdaptiveU32);
        assert!(report.is_monotonic);
        assert!(report.estimated_compressed_bytes < report.raw_bytes);
    }

    #[test]
    fn test_profile_runs_recommends_rle() {
        let mut rle_data = Vec::new();
        for _ in 0..10 {
            rle_data.extend(vec![42; 50]);
            rle_data.extend(vec![99; 50]);
        }
        let report = DataProfiler::profile_u32("status", 0, &rle_data);
        assert_eq!(report.recommended_codec, ColumnCodecTag::Rle);
        assert!(report.avg_run_length >= 25.0);
        assert!(report.estimated_compressed_bytes < report.raw_bytes);
    }

    #[test]
    fn test_profile_categorical_recommends_dictionary() {
        let mut dict_data = Vec::new();
        for i in 0..1000 {
            dict_data.push((i % 5) as u32);
        }
        let report = DataProfiler::profile_u32("cat", 0, &dict_data);
        assert_eq!(report.recommended_codec, ColumnCodecTag::Dictionary);
        assert_eq!(report.unique_count, 5);
        assert!(report.estimated_compressed_bytes < report.raw_bytes);
    }

    #[test]
    fn test_profile_multi_column_csv_and_codegen() {
        let csv = "ip_from,count,country,lat\n\
                   1000,10,US,37.5\n\
                   2000,10,DE,48.2\n\
                   3000,10,US,37.5\n\
                   4000,10,FR,48.8\n";

        let report = DataProfiler::profile_csv(csv, ',', true).unwrap();
        assert_eq!(report.total_rows, 4);
        assert_eq!(report.total_columns, 4);

        // ip_from is monotonic
        assert_eq!(
            report.columns[0].recommended_codec,
            ColumnCodecTag::AdaptiveU32
        );
        assert!(report.columns[0].is_monotonic);

        // country is categorical string
        assert_eq!(
            report.columns[2].recommended_codec,
            ColumnCodecTag::Dictionary
        );

        // lat is f32
        assert_eq!(
            report.columns[3].recommended_codec,
            ColumnCodecTag::GorillaF32
        );

        // Verify TOML export
        let toml = report.to_toml_config();
        assert!(toml.contains("[dataset]"));
        assert!(toml.contains("name = \"ip_from\""));

        // Verify Rust CodeGen
        let rust_code = report.to_rust_struct("GeoIntervalRecord");
        assert!(rust_code.contains("pub struct GeoIntervalRecord"));
        assert!(rust_code.contains("pub ip_from: u32"));
        assert!(rust_code.contains("pub country: String"));
        assert!(rust_code.contains("pub lat: f32"));
    }
}
