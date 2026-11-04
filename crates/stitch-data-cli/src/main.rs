//! Autonomous Lightweight Data Profiling & Binary Inspection CLI (`stitch-data`).

pub mod inspect;
pub mod profile;

use clap::{Parser, Subcommand};
use inspect::ContainerInspector;
use profile::DataProfiler;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "stitch-data",
    bin_name = "stitch-data",
    version,
    about = "Lightweight Autonomous Data Profiler & Zero-Copy Container Inspector",
    long_about = "Analyzes column entropy, recommends zero-waste codecs, and inspects Stitch Data containers."
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
    /// Heuristic column profiling and codec recommendation from CSV or raw numbers
    Profile {
        /// Target text/csv file with numbers or raw integers
        file: PathBuf,
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
        Commands::Profile { file } => match std::fs::read_to_string(&file) {
            Ok(content) => {
                let mut values = Vec::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Ok(v) = trimmed.parse::<u32>() {
                        values.push(v);
                    }
                }

                if values.is_empty() {
                    eprintln!("No parseable integers found in {}", file.display());
                    std::process::exit(1);
                }

                let report = DataProfiler::profile_u32(&values);
                println!("==========================================================");
                println!(" Stitch Data Autonomous Column Profiler");
                println!("==========================================================");
                println!(" Analyzed File:         {}", file.display());
                println!(" Total Sample Rows:     {}", report.total_count);
                println!(" Unique Values:         {}", report.unique_count);
                println!(" Monotonic Order:       {}", report.is_monotonic);
                println!(" Average Run Length:    {:.2}", report.avg_run_length);
                println!(" Effective Bit Width:   {} bits", report.bit_width);
                println!(
                    " Min .. Max Value:      {} .. {}",
                    report.min_value, report.max_value
                );
                println!("----------------------------------------------------------");
                println!(" RECOMMENDED CODEC:     {:?}", report.recommended_codec);
                println!(" JUSTIFICATION:         {}", report.reason);
                println!("==========================================================");
            }
            Err(e) => {
                eprintln!("Failed to read {}: {e}", file.display());
                std::process::exit(1);
            }
        },
    }
}
