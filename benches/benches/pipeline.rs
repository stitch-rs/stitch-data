use core::hint::black_box;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use cbp::prelude::*;
use cbp_benches::scenarios::*;

fn bench_pipeline_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("Pipeline/Monomorphic_U_Cycle");

    let sizes: &[usize] = &[16, 256, 4096, 65536, 1048576];

    for &n in sizes {
        let raw_bytes = (n * 4) as u64;
        group.throughput(Throughput::Bytes(raw_bytes));

        let input = generate_monotone_u32(n, 1000, 4);
        let mut scratch = vec![0u32; n];
        let mut out_buf = vec![0u8; n * 4 + 64];

        let mut pipeline = CompressionPipeline::new();
        let bytes_written = pipeline
            .compress_u32(&input, &mut scratch, &mut out_buf)
            .unwrap();

        let label = match n {
            16 => "N=16_64B_L1Line",
            256 => "N=256_1KiB_L1Cache",
            4096 => "N=4096_16KiB_L1dBound",
            65536 => "N=65536_256KiB_L2Bound",
            _ => "N=1048576_4MiB_L3Bound",
        };

        // Compress
        group.bench_with_input(BenchmarkId::new("compress_u32", label), &n, |b, _| {
            let mut p = CompressionPipeline::new();
            b.iter(|| {
                p.compress_u32(
                    black_box(&input),
                    black_box(&mut scratch),
                    black_box(&mut out_buf),
                )
                .unwrap()
            })
        });

        // Decompress
        let mut restored = vec![0u32; n];
        group.bench_with_input(BenchmarkId::new("decompress_u32", label), &n, |b, _| {
            b.iter(|| {
                DecompressionPipeline::decompress_u32(
                    black_box(&out_buf[..bytes_written]),
                    black_box(&mut restored),
                )
                .unwrap()
            })
        });
    }

    group.finish();
}

fn bench_profile_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("Pipeline/Profile_Comparison");
    let n = 1024;
    group.throughput(Throughput::Bytes((n * 4) as u64));

    let input = generate_monotone_u32(n, 1000, 2);
    let mut scratch = vec![0u32; n];
    let mut out_buf = vec![0u8; n * 4 + 64];

    let profiles = [
        (OptimizationProfile::Direct, "Direct_O0"),
        (OptimizationProfile::Realtime, "Realtime_O1"),
        (OptimizationProfile::Balanced, "Balanced_O2"),
        (OptimizationProfile::MaxDensity, "MaxDensity_O3"),
    ];

    for &(profile, label) in &profiles {
        let mut p = CompressionPipeline::new();
        let written = p
            .compress_with_profile_u32(profile, &input, &mut scratch, &mut out_buf)
            .unwrap();

        group.bench_function(format!("compress/{}", label), |b| {
            let mut p = CompressionPipeline::new();
            let mut s = vec![0u32; n];
            let mut out = vec![0u8; n * 4 + 64];
            b.iter(|| {
                p.compress_with_profile_u32(
                    profile,
                    black_box(&input),
                    black_box(&mut s),
                    black_box(&mut out),
                )
                .unwrap()
            })
        });

        group.bench_function(format!("decompress/{}", label), |b| {
            let mut dst = vec![0u32; n];
            b.iter(|| {
                DecompressionPipeline::decompress_with_profile_u32(
                    profile,
                    black_box(&out_buf[..written]),
                    black_box(&mut dst),
                )
                .unwrap()
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_pipeline_scaling, bench_profile_comparison);
criterion_main!(benches);
