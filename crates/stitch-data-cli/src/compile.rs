//! Universal CSV to Stitch Data Container (.stdc) Pack & Compile Engine.
//!
//! Streams arbitrary CSV rows, maps columns through the recommended columnar codecs,
//! packs blocks into framed containers, and writes index directories with Castagnoli CRC32-C.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use stitch_data::columnar::{encode_raw_column, encode_u32_column};
use stitch_data::frame::{FrameFlags, FrameHeader, OptimizationProfile, pack_frame};
use stitch_data::{ColumnCodecTag, ColumnarBlockWriter, ContainerWriter};

use crate::profile::{DataProfiler, InferredDataType};

pub struct DatasetCompiler;

impl DatasetCompiler {
    /// Compiles a CSV file into a `.stdc` binary container.
    pub fn compile_csv(
        csv_path: &Path,
        out_path: &Path,
        delimiter: char,
        has_header: bool,
        chunk_size: usize,
    ) -> Result<usize, String> {
        let content = std::fs::read_to_string(csv_path)
            .map_err(|e| format!("Failed to read {}: {e}", csv_path.display()))?;

        // 1. Profile schema and recommend codecs
        let report = DataProfiler::profile_csv(&content, delimiter, has_header)
            .map_err(|e| format!("Profiling error: {e}"))?;

        // 2. Parse all rows into memory for chunking
        let mut lines = content.lines().filter(|l| !l.trim().is_empty());
        if has_header {
            lines.next();
        }

        let mut rows: Vec<Vec<String>> = Vec::new();
        for line in lines {
            let cols: Vec<String> = line
                .split(delimiter)
                .map(|s| s.trim().to_string())
                .collect();
            if cols.len() == report.total_columns {
                rows.push(cols);
            }
        }

        let total_rows = rows.len();
        if total_rows == 0 {
            return Err("No valid data rows found in CSV".to_string());
        }

        let out_file = File::create(out_path)
            .map_err(|e| format!("Failed to create {}: {e}", out_path.display()))?;
        let mut writer = BufWriter::new(out_file);

        let mut container_writer = ContainerWriter::new(&mut writer, 0x5444_435F_0001, 0)
            .map_err(|e| format!("Failed to init container writer: {e}"))?;

        let mut payload_buf = vec![0u8; chunk_size * report.total_columns * 8 + 1024];
        let mut frame_buf = vec![0u8; payload_buf.len() + 256];
        let mut scratch = vec![0u32; chunk_size];
        let mut col_buf = vec![0u8; chunk_size * 5 + 64];

        for (chunk_idx, chunk_rows) in rows.chunks(chunk_size).enumerate() {
            let row_count = chunk_rows.len();
            let mut block_writer =
                ColumnarBlockWriter::new(row_count, report.total_columns, &mut payload_buf)
                    .map_err(|e| format!("BlockWriter error: {e:?}"))?;

            for col_idx in 0..report.total_columns {
                let col_desc = &report.columns[col_idx];
                match col_desc.data_type {
                    InferredDataType::U32 => {
                        let col_vals: Vec<u32> = chunk_rows
                            .iter()
                            .map(|r| r[col_idx].parse::<u32>().unwrap_or(0))
                            .collect();
                        let col_len = encode_u32_column(&col_vals, &mut scratch, &mut col_buf)
                            .map_err(|e| format!("Encode error: {e:?}"))?;
                        block_writer
                            .write_column(ColumnCodecTag::AdaptiveU32, &col_buf[..col_len])
                            .map_err(|e| format!("Write column error: {e:?}"))?;
                    }
                    _ => {
                        // Raw byte representation fallback
                        let mut raw_bytes = Vec::new();
                        for r in chunk_rows {
                            let b = r[col_idx].as_bytes();
                            raw_bytes.extend_from_slice(b);
                        }
                        let col_len = encode_raw_column(&raw_bytes, &mut col_buf)
                            .map_err(|e| format!("Encode raw error: {e:?}"))?;
                        block_writer
                            .write_column(ColumnCodecTag::Raw, &col_buf[..col_len])
                            .map_err(|e| format!("Write column error: {e:?}"))?;
                    }
                }
            }

            let payload_len = block_writer
                .finish()
                .map_err(|e| format!("Finish block error: {e:?}"))?;
            let header = FrameHeader {
                profile: OptimizationProfile::Balanced,
                flags: FrameFlags::default(),
                payload_len,
                schema_hash: None,
            };
            let frame_len = pack_frame(&header, &payload_buf[..payload_len], &mut frame_buf)
                .map_err(|e| format!("Pack frame error: {e:?}"))?;

            let first_key = chunk_idx as u64 * chunk_size as u64;
            container_writer
                .write_chunk(first_key, row_count, &frame_buf[..frame_len])
                .map_err(|e| format!("Write chunk error: {e}"))?;
        }

        container_writer
            .finish()
            .map_err(|e| format!("Container finish error: {e}"))?;

        writer.flush().map_err(|e| format!("Flush error: {e}"))?;
        Ok(total_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stitch_data::ContainerReader;

    #[test]
    fn test_compile_csv_to_container_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let csv_path = temp_dir.path().join("test_input.csv");
        let out_path = temp_dir.path().join("test_out.stdc");

        let csv_content = "id,value\n\
                           100,500\n\
                           101,500\n\
                           102,501\n\
                           103,502\n";
        std::fs::write(&csv_path, csv_content).unwrap();

        let rows = DatasetCompiler::compile_csv(&csv_path, &out_path, ',', true, 2).unwrap();
        assert_eq!(rows, 4);

        // Verify with ContainerReader
        let bytes = std::fs::read(&out_path).unwrap();
        let reader = ContainerReader::open(&bytes).unwrap();
        assert_eq!(reader.chunk_count(), 2);
    }
}
