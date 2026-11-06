use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use stitch_data::CacheSuperBlock;

/// Unaligned 24-byte record (causes cache-line straddling across 64-byte boundaries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct UnalignedRecord24 {
    key: u64,
    offset: u64,
    len: u32,
    count: u32,
}

/// 16-byte compact record (perfect power-of-two divisor: 4 records per 64-byte line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct CompactRecord16 {
    key: u64,
    offset: i32,
    len_and_count: u32,
}

fn bench_sequential_scan(c: &mut Criterion) {
    const NUM_RECORDS: usize = 16_384; // ~384 KB (spills L1/L2 caches to test memory bus throughput)

    // 1. Unaligned 24B records
    let unaligned: Vec<UnalignedRecord24> = (0..NUM_RECORDS)
        .map(|i| UnalignedRecord24 {
            key: i as u64,
            offset: (i * 100) as u64,
            len: 1024,
            count: 50,
        })
        .collect();

    // 2. SuperBlock tiled 24B records (8 items = 192B = exactly 3 cache lines, 0 straddle boundaries)
    let superblocks: Vec<CacheSuperBlock<UnalignedRecord24, 8>> = (0..NUM_RECORDS / 8)
        .map(|b| {
            let base = b * 8;
            let items = std::array::from_fn(|i| UnalignedRecord24 {
                key: (base + i) as u64,
                offset: ((base + i) * 100) as u64,
                len: 1024,
                count: 50,
            });
            CacheSuperBlock::new(items)
        })
        .collect();

    // 3. Compact 16B records
    let compact: Vec<CompactRecord16> = (0..NUM_RECORDS)
        .map(|i| CompactRecord16 {
            key: i as u64,
            offset: (i * 100) as i32,
            len_and_count: 1024,
        })
        .collect();

    let mut group = c.benchmark_group("cache_layout/sequential_scan");

    group.bench_function("unaligned_24b_straddling", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for rec in black_box(&unaligned) {
                sum = sum.wrapping_add(rec.key ^ rec.offset);
            }
            sum
        });
    });

    group.bench_function("superblock_24b_tiled_192b", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for block in black_box(&superblocks) {
                for rec in block.as_slice() {
                    sum = sum.wrapping_add(rec.key ^ rec.offset);
                }
            }
            sum
        });
    });

    group.bench_function("compact_16b_perfect_divisor", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for rec in black_box(&compact) {
                sum = sum.wrapping_add(rec.key ^ (rec.offset as u64));
            }
            sum
        });
    });

    group.finish();
}

fn bench_random_lookup(c: &mut Criterion) {
    const NUM_RECORDS: usize = 16_384;
    const NUM_QUERIES: usize = 1_024;

    let unaligned: Vec<UnalignedRecord24> = (0..NUM_RECORDS)
        .map(|i| UnalignedRecord24 {
            key: i as u64,
            offset: (i * 100) as u64,
            len: 1024,
            count: 50,
        })
        .collect();

    let superblocks: Vec<CacheSuperBlock<UnalignedRecord24, 8>> = (0..NUM_RECORDS / 8)
        .map(|b| {
            let base = b * 8;
            let items = std::array::from_fn(|i| UnalignedRecord24 {
                key: (base + i) as u64,
                offset: ((base + i) * 100) as u64,
                len: 1024,
                count: 50,
            });
            CacheSuperBlock::new(items)
        })
        .collect();

    let compact: Vec<CompactRecord16> = (0..NUM_RECORDS)
        .map(|i| CompactRecord16 {
            key: i as u64,
            offset: (i * 100) as i32,
            len_and_count: 1024,
        })
        .collect();

    // Pseudo-random query indices
    let mut rng_seed: u32 = 0xDEAD_BEEF;
    let query_indices: Vec<usize> = (0..NUM_QUERIES)
        .map(|_| {
            rng_seed = rng_seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (rng_seed as usize) % NUM_RECORDS
        })
        .collect();

    let mut group = c.benchmark_group("cache_layout/random_lookup");

    group.bench_function("unaligned_24b_straddling", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for &idx in black_box(&query_indices) {
                let rec = &unaligned[idx];
                sum = sum.wrapping_add(rec.key ^ rec.offset);
            }
            sum
        });
    });

    group.bench_function("superblock_24b_tiled_192b", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for &idx in black_box(&query_indices) {
                let block_idx = idx / 8;
                let item_idx = idx % 8;
                let rec = &superblocks[block_idx][item_idx];
                sum = sum.wrapping_add(rec.key ^ rec.offset);
            }
            sum
        });
    });

    group.bench_function("compact_16b_perfect_divisor", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for &idx in black_box(&query_indices) {
                let rec = &compact[idx];
                sum = sum.wrapping_add(rec.key ^ (rec.offset as u64));
            }
            sum
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sequential_scan, bench_random_lookup);
criterion_main!(benches);
