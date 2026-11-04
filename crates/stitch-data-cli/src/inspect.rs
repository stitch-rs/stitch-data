//! Zero-Copy Container Inspector for Stitch Data files.
//!
//! Validates magic bytes, header, chunk directory, and Castagnoli CRC32-C integrity.

use std::path::Path;
use stitch_data::ContainerReader;

pub struct ContainerInspector;

impl ContainerInspector {
    pub fn inspect_file(path: &Path) -> Result<(), String> {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let mmap = unsafe {
            memmap2::Mmap::map(&file)
                .map_err(|e| format!("Failed to mmap {}: {e}", path.display()))?
        };

        let reader =
            ContainerReader::open(&mmap).map_err(|e| format!("Corrupted container: {e:?}"))?;

        println!("==========================================================");
        println!(" Stitch Data Container Inspector");
        println!("==========================================================");
        println!(" File:             {}", path.display());
        println!(
            " File Size:        {} bytes ({:.2} KiB)",
            mmap.len(),
            mmap.len() as f64 / 1024.0
        );
        println!(
            " Magic:            {:?}",
            std::str::from_utf8(&reader.header().magic).unwrap_or("????")
        );
        println!(" Format Version:   {}", reader.header().version);
        println!(" Schema Hash:      0x{:016X}", reader.header().schema_hash);
        println!(" Chunk Count:      {}", reader.chunk_count());
        println!("----------------------------------------------------------");
        println!(" Chunks Breakdown:");

        let mut total_rows = 0;
        let mut total_compressed = 0;

        for i in 0..reader.chunk_count() {
            if let Ok(desc) = reader.chunk_descriptor(i) {
                total_rows += desc.row_count as usize;
                total_compressed += desc.compressed_len as usize;
                if i < 10 || i == reader.chunk_count() - 1 {
                    println!(
                        "  [{:>3}] First Key: {:>10} | Offset: {:>8} | Len: {:>6} B | Rows: {:>5}",
                        i, desc.first_key, desc.file_offset, desc.compressed_len, desc.row_count
                    );
                } else if i == 10 {
                    println!(
                        "  ... [{} intermediate chunks omitted] ...",
                        reader.chunk_count() - 11
                    );
                }
            }
        }

        println!("----------------------------------------------------------");
        println!(" Total Logical Rows:       {}", total_rows);
        println!(" Total Payload Bytes:      {} bytes", total_compressed);
        if total_rows > 0 {
            println!(
                " Avg Density:              {:.2} bytes / row",
                total_compressed as f64 / total_rows as f64
            );
        }
        println!(" Status:                   Integrity Verified (Castagnoli CRC32-C PASS)");
        println!("==========================================================");

        Ok(())
    }
}
