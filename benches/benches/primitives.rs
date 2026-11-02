use core::hint::black_box;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use stitch_data::prelude::*;

fn bench_prefix_varint(c: &mut Criterion) {
    let mut group = c.benchmark_group("Primitives/PrefixVarint");

    let test_values: &[(u64, &str)] = &[
        (0, "0_1B"),
        (1, "1_1B"),
        (127, "127_1B"),
        (128, "128_2B"),
        (16383, "16383_2B"),
        (16384, "16384_3B"),
        ((1 << 21) - 1, "2097151_3B"),
        (1 << 21, "2097152_4B"),
        ((1 << 28) - 1, "268435455_4B"),
        (1 << 28, "268435456_5B"),
        (u64::MAX, "u64_max_9B"),
    ];

    for &(val, label) in test_values {
        let mut buf = [0u8; 9];
        let encoded_len = encode_prefix_varint(val, &mut buf);

        group.bench_function(format!("encode/{}", label), |b| {
            let mut out = [0u8; 9];
            b.iter(|| encode_prefix_varint(black_box(val), black_box(&mut out)))
        });

        group.bench_function(format!("decode/{}", label), |b| {
            b.iter(|| decode_prefix_varint(black_box(&buf[..encoded_len])).unwrap())
        });
    }

    group.finish();
}

fn bench_zigzag(c: &mut Criterion) {
    let mut group = c.benchmark_group("Primitives/ZigZag");

    let i32_cases: &[(i32, &str)] = &[
        (0, "0"),
        (-1, "neg_1"),
        (1, "pos_1"),
        (i32::MIN, "i32_min"),
        (i32::MAX, "i32_max"),
    ];

    for &(val, label) in i32_cases {
        let encoded = encode_zigzag32(val);
        group.bench_function(format!("zigzag32/encode/{}", label), |b| {
            b.iter(|| encode_zigzag32(black_box(val)))
        });
        group.bench_function(format!("zigzag32/decode/{}", label), |b| {
            b.iter(|| decode_zigzag32(black_box(encoded)))
        });
    }

    let i64_cases: &[(i64, &str)] = &[
        (0, "0"),
        (-1, "neg_1"),
        (1, "pos_1"),
        (i64::MIN, "i64_min"),
        (i64::MAX, "i64_max"),
    ];

    for &(val, label) in i64_cases {
        let encoded = encode_zigzag64(val);
        group.bench_function(format!("zigzag64/encode/{}", label), |b| {
            b.iter(|| encode_zigzag64(black_box(val)))
        });
        group.bench_function(format!("zigzag64/decode/{}", label), |b| {
            b.iter(|| decode_zigzag64(black_box(encoded)))
        });
    }

    group.finish();
}

fn bench_crc32(c: &mut Criterion) {
    let mut group = c.benchmark_group("Primitives/CRC32_Castagnoli");

    let sizes: &[usize] = &[8, 24, 64, 1024, 65536, 1048576];

    for &size in sizes {
        let data = vec![0xABu8; size];
        group.throughput(Throughput::Bytes(size as u64));

        let label = match size {
            8 => "8B".to_string(),
            24 => "24B".to_string(),
            64 => "64B".to_string(),
            1024 => "1KiB".to_string(),
            65536 => "64KiB".to_string(),
            _ => "1MiB".to_string(),
        };

        group.bench_function(format!("crc32/{}", label), |b| {
            b.iter(|| crc32_castagnoli(black_box(&data)))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_prefix_varint, bench_zigzag, bench_crc32);
criterion_main!(benches);
