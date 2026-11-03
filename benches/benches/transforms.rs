use core::hint::black_box;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use stitch_data::prelude::*;
use stitch_data_benches::scenarios::*;

type GenFn = fn(usize) -> Vec<u32>;

fn bench_delta_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("Transforms/Delta");

    let patterns: &[(&str, GenFn)] = &[
        ("monotone_step_1", |n| generate_monotone_u32(n, 0, 1)),
        ("monotone_step_256", |n| generate_monotone_u32(n, 0, 256)),
        ("constant", |n| generate_monotone_u32(n, 42, 0)),
        ("random", |n| generate_bounded_u32(n, u32::MAX, 0x1234_5678)),
    ];

    for &(name, gen_fn) in patterns {
        for &n in &[1024usize, 1048576usize] {
            let label = if n == 1024 { "1K" } else { "1M" };
            let raw_bytes = (n * 4) as u64;
            group.throughput(Throughput::Bytes(raw_bytes));

            let data = gen_fn(n);

            group.bench_function(format!("encode/{}/{}", name, label), |b| {
                b.iter_batched(
                    || data.clone(),
                    |mut buf| {
                        delta_encode_u32(black_box(&mut buf));
                        black_box(buf[0])
                    },
                    criterion::BatchSize::SmallInput,
                )
            });
        }
    }

    // Signed i32 delta bench
    let n = 1024;
    group.throughput(Throughput::Bytes((n * 4) as u64));
    let mut signed_data = vec![0i32; n];
    for (i, item) in signed_data.iter_mut().enumerate() {
        *item = (i as i32 * 3) - 500;
    }
    group.bench_function("encode/signed_i32/1K", |b| {
        b.iter_batched(
            || signed_data.clone(),
            |mut buf| {
                delta_encode_i32(black_box(&mut buf));
                black_box(buf[0])
            },
            criterion::BatchSize::SmallInput,
        )
    });
    let mut encoded_signed = signed_data.clone();
    delta_encode_i32(&mut encoded_signed);
    group.bench_function("decode/signed_i32/1K", |b| {
        b.iter_batched(
            || encoded_signed.clone(),
            |mut buf| {
                delta_decode_i32(black_box(&mut buf));
                black_box(buf[0])
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn bench_bitpack_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("Transforms/BitPack");
    let n = 1024;
    group.throughput(Throughput::Bytes((n * 4) as u64));

    let bit_widths: &[u8] = &[1, 4, 8, 12, 16, 32];

    for &w in bit_widths {
        let max_val = if w == 32 { u32::MAX } else { (1u32 << w) - 1 };
        let input = generate_bounded_u32(n, max_val, 0xABCD_EF01);
        let mut packed = vec![0u8; n * 4 + 64];
        let bytes_written = BitPacker::pack_u32(&input, &mut packed).unwrap();

        group.bench_function(format!("pack_u32/width_{}b", w), |b| {
            let mut out = vec![0u8; n * 4 + 64];
            b.iter(|| BitPacker::pack_u32(black_box(&input), black_box(&mut out)).unwrap())
        });

        group.bench_function(format!("unpack_u32/width_{}b", w), |b| {
            let mut dst = vec![0u32; n];
            b.iter(|| {
                BitPacker::unpack_u32(black_box(&packed[..bytes_written]), black_box(&mut dst))
                    .unwrap()
            })
        });
    }

    group.finish();
}

fn bench_rle_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("Transforms/RLE");
    let n = 1024;
    group.throughput(Throughput::Bytes((n * 4) as u64));

    let run_configs: &[(usize, &str)] = &[(256, "run_256"), (16, "run_16"), (1, "run_1_unique")];

    for &(run_len, label) in run_configs {
        let input = generate_rle_u32(n, run_len, 0x55AA_55AA);
        let mut encoded = vec![0u8; n * 6 + 128];
        let encoded_len = RleTransformer::encode_u32(&input, &mut encoded).unwrap();

        group.bench_function(format!("encode_u32/{}", label), |b| {
            let mut out = vec![0u8; n * 6 + 128];
            b.iter(|| RleTransformer::encode_u32(black_box(&input), black_box(&mut out)).unwrap())
        });

        group.bench_function(format!("decode_u32/{}", label), |b| {
            let mut dst = vec![0u32; n];
            b.iter(|| {
                RleTransformer::decode_u32(black_box(&encoded[..encoded_len]), black_box(&mut dst))
                    .unwrap()
            })
        });
    }

    group.finish();
}

fn bench_gorilla_float(c: &mut Criterion) {
    let mut group = c.benchmark_group("Transforms/Gorilla_Float");
    let n = 1024;

    // f32 trajectory: 4 KiB raw
    group.throughput(Throughput::Bytes((n * 4) as u64));
    let mut f32_data = vec![0.0f32; n];
    let mut val = 100.0f32;
    for item in &mut f32_data {
        val += 0.05;
        *item = val;
    }
    let mut out_f32 = vec![0u8; n * 4 + 64];
    let written_f32 = GorillaCompressor::compress_f32(&f32_data, &mut out_f32).unwrap();

    group.bench_function("compress_f32/trajectory_1K", |b| {
        let mut out = vec![0u8; n * 4 + 64];
        b.iter(|| {
            GorillaCompressor::compress_f32(black_box(&f32_data), black_box(&mut out)).unwrap()
        })
    });

    group.bench_function("decompress_f32/trajectory_1K", |b| {
        let mut dst = vec![0.0f32; n];
        b.iter(|| {
            GorillaCompressor::decompress_f32(
                black_box(&out_f32[..written_f32]),
                black_box(&mut dst),
            )
            .unwrap()
        })
    });

    // f64 trajectory: 8 KiB raw
    group.throughput(Throughput::Bytes((n * 8) as u64));
    let mut f64_data = vec![0.0f64; n];
    let mut val64 = 59.934280f64;
    for item in &mut f64_data {
        val64 += 0.000005;
        *item = val64;
    }
    let mut out_f64 = vec![0u8; n * 8 + 64];
    let written_f64 = GorillaCompressor::compress_f64(&f64_data, &mut out_f64).unwrap();

    group.bench_function("compress_f64/trajectory_1K", |b| {
        let mut out = vec![0u8; n * 8 + 64];
        b.iter(|| {
            GorillaCompressor::compress_f64(black_box(&f64_data), black_box(&mut out)).unwrap()
        })
    });

    group.bench_function("decompress_f64/trajectory_1K", |b| {
        let mut dst = vec![0.0f64; n];
        b.iter(|| {
            GorillaCompressor::decompress_f64(
                black_box(&out_f64[..written_f64]),
                black_box(&mut dst),
            )
            .unwrap()
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_delta_transform,
    bench_bitpack_transform,
    bench_rle_transform,
    bench_gorilla_float,
);
criterion_main!(benches);
