//! Autonomous Lightweight Data Profiling & Binary Inspection CLI (`stitch-data`).

pub mod compile;
pub mod inspect;
pub mod profile;
pub mod simulate;

use clap::{Parser, Subcommand};
use compile::DatasetCompiler;
use inspect::ContainerInspector;
use profile::DataProfiler;
use simulate::CodecSimulator;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "stitch-data",
    bin_name = "stitch-data",
    version,
    about = "Lightweight Autonomous Data Profiler & Zero-Copy Container Inspector",
    long_about = "Analyzes dataset schema, entropy, recommends zero-waste codecs, simulates codecs, and packs Stitch Data containers."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Zero-copy binary container inspection and CRC verification
    Inspect {
        /// Target `.bin` or `.stdc` container file
        file: PathBuf,
    },
    /// Heuristic column profiling and codec recommendation from CSV/TSV or raw numbers
    Profile {
        /// Target CSV, TSV, or raw text file
        file: PathBuf,
        /// CSV delimiter character (default: ',')
        #[arg(short, long, default_value_t = ',')]
        delimiter: char,
        /// Treat first row as column header names
        #[arg(long, default_value_t = true)]
        header: bool,
        /// Emit code/config presets: "terminal", "toml", or "rust"
        #[arg(short, long, default_value = "terminal")]
        format: String,
        /// Name of generated Rust struct (when format=rust)
        #[arg(long, default_value = "GeneratedRecord")]
        struct_name: String,
    },
    /// What-If simulation benchmarking all candidate codecs on raw integer streams
    Simulate {
        /// Target file containing raw integers
        file: PathBuf,
    },
    /// Packs and compiles an arbitrary CSV file directly into a `.stdc` container
    Pack {
        /// Input CSV file
        #[arg(short, long)]
        input: PathBuf,
        /// Output `.stdc` container file
        #[arg(short, long)]
        output: PathBuf,
        /// CSV delimiter character (default: ',')
        #[arg(short, long, default_value_t = ',')]
        delimiter: char,
        /// Has header row
        #[arg(long, default_value_t = true)]
        header: bool,
        /// Block chunk size in rows (default: 64)
        #[arg(long, default_value_t = 64)]
        chunk_size: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { file } => {
            if let Err(e) = ContainerInspector::inspect_file(&file) {
                eprintln!("Error inspecting {}: {e}", file.display());
                std::process::exit(1);
            }
        }
        Commands::Simulate { file } => match std::fs::read_to_string(&file) {
            Ok(content) => {
                let mut values = Vec::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Ok(v) = trimmed.parse::<u32>() {
                        values.push(v);
                    }
                }
                if values.is_empty() {
                    eprintln!("No parseable u32 integers found in {}", file.display());
                    std::process::exit(1);
                }
                let results = CodecSimulator::simulate_u32(&values);
                println!(
                    "================================================================================"
                );
                println!(" Stitch Data What-If Codec Simulator");
                println!(
                    "================================================================================"
                );
                println!(" Sample Input:       {}", file.display());
                println!(" Total Values:       {}", values.len());
                println!(" Raw Uncompressed:   {} bytes", values.len() * 4);
                println!(
                    "--------------------------------------------------------------------------------"
                );
                println!(
                    "{:<30} {:<12} {:<10} {:<12} {:<12}",
                    "CODEC", "BYTES", "RATIO", "ENC (µs)", "DEC (µs)"
                );
                println!(
                    "--------------------------------------------------------------------------------"
                );
                for r in results {
                    println!(
                        "{:<30} {:<12} {:>6.2}%   {:>8.2}     {:>8.2}",
                        r.name,
                        r.compressed_bytes,
                        r.compression_ratio * 100.0,
                        r.encode_time_micros,
                        r.decode_time_micros
                    );
                }
                println!(
                    "================================================================================"
                );
            }
            Err(e) => {
                eprintln!("Failed to read {}: {e}", file.display());
                std::process::exit(1);
            }
        },
        Commands::Pack {
            input,
            output,
            delimiter,
            header,
            chunk_size,
        } => {
            println!("Packing {} -> {}...", input.display(), output.display());
            match DatasetCompiler::compile_csv(&input, &output, delimiter, header, chunk_size) {
                Ok(rows) => {
                    println!(
                        "Successfully compiled {} rows into {}!",
                        rows,
                        output.display()
                    );
                }
                Err(e) => {
                    eprintln!("Compilation error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Profile {
            file,
            delimiter,
            header,
            format,
            struct_name,
        } => match std::fs::read_to_string(&file) {
            Ok(content) => match DataProfiler::profile_csv(&content, delimiter, header) {
                Ok(dataset) => match format.to_lowercase().as_str() {
                    "toml" => {
                        println!("{}", dataset.to_toml_config());
                    }
                    "rust" => {
                        println!("{}", dataset.to_rust_struct(&struct_name));
                    }
                    _ => {
                        println!(
                            "================================================================================"
                        );
                        println!(" Stitch Data Autonomous Column Profiler & Auto-Tuner");
                        println!(
                            "================================================================================"
                        );
                        println!(" Analyzed File:         {}", file.display());
                        println!(" Total Rows Sampled:    {}", dataset.total_rows);
                        println!(" Total Columns:         {}", dataset.total_columns);
                        println!(
                            " Uncompressed Size:     {:.2} KB ({} bytes)",
                            dataset.total_raw_bytes as f64 / 1024.0,
                            dataset.total_raw_bytes
                        );
                        println!(
                            " Estimated Compressed:  {:.2} KB ({} bytes)",
                            dataset.total_estimated_compressed_bytes as f64 / 1024.0,
                            dataset.total_estimated_compressed_bytes
                        );
                        println!(
                            " Estimated Space Gain:  {:.1}% net reduction",
                            (1.0 - dataset.compression_ratio()) * 100.0
                        );
                        println!(
                            "--------------------------------------------------------------------------------"
                        );
                        println!(
                            "{:<4} {:<16} {:<12} {:<16} {:<8} {:<12}",
                            "IDX", "COLUMN", "TYPE", "RECOMMENDED", "WIDTH", "SAVINGS"
                        );
                        println!(
                            "--------------------------------------------------------------------------------"
                        );
                        for col in &dataset.columns {
                            let saving_pct = if col.raw_bytes == 0 {
                                0.0
                            } else {
                                (1.0 - (col.estimated_compressed_bytes as f64
                                    / col.raw_bytes as f64))
                                    * 100.0
                            };
                            println!(
                                "{:<4} {:<16} {:<12?} {:<16?} {:<8} {:>5.1}%",
                                col.index,
                                col.name,
                                col.data_type,
                                col.recommended_codec,
                                format!("{}b", col.bit_width),
                                saving_pct
                            );
                            println!("     └─ {}", col.reason);
                        }
                        println!(
                            "================================================================================"
                        );
                        println!(
                            " Tip: use `--format toml` to export configuration, or `--format rust` for codegen."
                        );
                    }
                },
                Err(e) => {
                    eprintln!("Failed to profile {}: {e}", file.display());
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Failed to read {}: {e}", file.display());
                std::process::exit(1);
            }
        },
    }
}
