use core::hint::black_box;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use prost::Message;
use serde::{Deserialize, Serialize};

use cbp::prelude::*;

// --- Domain Models ---

/// 1. IPAtlas Range Interval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv4Interval {
    pub ip_from: u32,
    pub ip_to: u32,
    pub profile_id: u32,
}

/// Protobuf-compatible Interval Model
#[derive(Clone, PartialEq, Message)]
pub struct ProtoIpv4IntervalList {
    #[prost(uint32, repeated, tag = "1")]
    pub ip_from: Vec<u32>,
    #[prost(uint32, repeated, tag = "2")]
    pub ip_to: Vec<u32>,
    #[prost(uint32, repeated, tag = "3")]
    pub profile_id: Vec<u32>,
}

/// 2. Game Telemetry Snapshot (Player State)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub player_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub health: u16,
    pub flags: u16,
    pub tick: u32,
}

/// Protobuf-compatible Player Snapshot
#[derive(Clone, PartialEq, Message)]
pub struct ProtoPlayerSnapshot {
    #[prost(uint32, tag = "1")]
    pub player_id: u32,
    #[prost(float, tag = "2")]
    pub x: f32,
    #[prost(float, tag = "3")]
    pub y: f32,
    #[prost(float, tag = "4")]
    pub z: f32,
    #[prost(uint32, tag = "5")]
    pub health: u32,
    #[prost(uint32, tag = "6")]
    pub flags: u32,
    #[prost(uint32, tag = "7")]
    pub tick: u32,
}

// --- Benchmark Functions ---

fn bench_ipatlas_intervals(c: &mut Criterion) {
    let count = 1024;
    // Generate monotonically increasing sequential IP intervals
    let mut ip_from = Vec::with_capacity(count);
    let mut ip_to = Vec::with_capacity(count);
    let mut profile_ids = Vec::with_capacity(count);

    let mut current_ip = 16777216u32; // 1.0.0.0
    for i in 0..count {
        let span = 256;
        ip_from.push(current_ip);
        ip_to.push(current_ip + span);
        profile_ids.push((i % 64) as u32);
        current_ip += span + 4;
    }

    let intervals: Vec<Ipv4Interval> = (0..count)
        .map(|i| Ipv4Interval {
            ip_from: ip_from[i],
            ip_to: ip_to[i],
            profile_id: profile_ids[i],
        })
        .collect();

    let proto_intervals = ProtoIpv4IntervalList {
        ip_from: ip_from.clone(),
        ip_to: ip_to.clone(),
        profile_id: profile_ids.clone(),
    };

    // Pre-encode and report sizes
    let bincode_bytes = bincode::serialize(&intervals).unwrap();
    let rmp_bytes = rmp_serde::to_vec(&intervals).unwrap();
    let postcard_bytes = postcard::to_allocvec(&intervals).unwrap();

    let mut proto_bytes = Vec::new();
    proto_intervals.encode(&mut proto_bytes).unwrap();

    // CBP Pipeline Encode
    let mut cbp_scratch = vec![0u32; count];
    let mut cbp_out = vec![0u8; count * 4 * 3 + 128];
    let mut cbp_pipeline = CompressionPipeline::new();

    let cbp_ip_from_len = cbp_pipeline
        .compress_u32(&ip_from, &mut cbp_scratch, &mut cbp_out)
        .unwrap();

    let raw_size = count * 12; // 12 bytes per interval = 12,288 bytes
    println!("\n=== IPAtlas 1024 Intervals (12 KB Raw) Compression Scoreboard ===");
    println!("Raw Size:     {:>6} bytes", raw_size);
    println!("Bincode:      {:>6} bytes (100.0%)", bincode_bytes.len());
    println!(
        "Postcard:     {:>6} bytes ({:.1}%)",
        postcard_bytes.len(),
        (postcard_bytes.len() as f64 / raw_size as f64) * 100.0
    );
    println!(
        "Protobuf:     {:>6} bytes ({:.1}%)",
        proto_bytes.len(),
        (proto_bytes.len() as f64 / raw_size as f64) * 100.0
    );
    println!(
        "MessagePack:  {:>6} bytes ({:.1}%)",
        rmp_bytes.len(),
        (rmp_bytes.len() as f64 / raw_size as f64) * 100.0
    );
    println!(
        "CBP (ip_from):{:>6} bytes (Delta+BitPack: {:.1}% of uncompressed ip_from)",
        cbp_ip_from_len,
        (cbp_ip_from_len as f64 / (count * 4) as f64) * 100.0
    );
    println!("==================================================================\n");

    let mut group = c.benchmark_group("IPAtlas Intervals Serialization");
    group.throughput(Throughput::Bytes(raw_size as u64));

    group.bench_function("Bincode/Serialize", |b| {
        b.iter(|| black_box(bincode::serialize(black_box(&intervals)).unwrap()))
    });

    group.bench_function("Postcard/Serialize", |b| {
        b.iter(|| black_box(postcard::to_allocvec(black_box(&intervals)).unwrap()))
    });

    group.bench_function("Protobuf/Encode", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(proto_bytes.len());
            black_box(&proto_intervals).encode(&mut buf).unwrap();
            black_box(buf)
        })
    });

    group.bench_function("CBP/Pipeline/Compress_u32", |b| {
        let mut scratch = vec![0u32; count];
        let mut out = vec![0u8; count * 4];
        let mut pipeline = CompressionPipeline::new();
        b.iter(|| {
            pipeline
                .compress_u32(
                    black_box(&ip_from),
                    black_box(&mut scratch),
                    black_box(&mut out),
                )
                .unwrap()
        })
    });

    group.finish();
}

fn bench_game_snapshot(c: &mut Criterion) {
    let snapshot = PlayerSnapshot {
        player_id: 1337,
        x: 1024.5,
        y: -512.25,
        z: 64.125,
        health: 100,
        flags: 0b0001_0010,
        tick: 128_000,
    };

    let proto_snapshot = ProtoPlayerSnapshot {
        player_id: 1337,
        x: 1024.5,
        y: -512.25,
        z: 64.125,
        health: 100,
        flags: 0b0001_0010,
        tick: 128_000,
    };

    let mut proto_buf = Vec::new();
    proto_snapshot.encode(&mut proto_buf).unwrap();

    let bincode_buf = bincode::serialize(&snapshot).unwrap();
    let postcard_buf = postcard::to_allocvec(&snapshot).unwrap();

    // CBP Frame Pack
    let header = FrameHeader {
        profile: OptimizationProfile::Realtime,
        flags: FrameFlags {
            is_compressed: false,
            is_chunked: false,
            has_schema_hash: false,
        },
        payload_len: bincode_buf.len(),
        schema_hash: None,
    };

    let mut cbp_buf = [0u8; 64];
    let cbp_len = pack_frame(&header, &bincode_buf, &mut cbp_buf).unwrap();

    println!("=== Game Tick Snapshot (24B) Size Scoreboard ===");
    println!("Bincode:     {:>3} bytes", bincode_buf.len());
    println!("Protobuf:    {:>3} bytes", proto_buf.len());
    println!("Postcard:    {:>3} bytes", postcard_buf.len());
    println!(
        "CBP Frame:   {:>3} bytes (Header + Payload + Castagnoli CRC32)",
        cbp_len
    );
    println!("=================================================\n");

    let mut group = c.benchmark_group("Game Snapshot Serialization");

    group.bench_function("Bincode/Serialize", |b| {
        b.iter(|| black_box(bincode::serialize(black_box(&snapshot)).unwrap()))
    });

    group.bench_function("Postcard/Serialize", |b| {
        b.iter(|| black_box(postcard::to_allocvec(black_box(&snapshot)).unwrap()))
    });

    group.bench_function("Protobuf/Encode", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(32);
            black_box(&proto_snapshot).encode(&mut buf).unwrap();
            black_box(buf)
        })
    });

    group.bench_function("CBP/Frame/Pack", |b| {
        let mut buf = [0u8; 64];
        b.iter(|| {
            pack_frame(
                black_box(&header),
                black_box(&bincode_buf),
                black_box(&mut buf),
            )
            .unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, bench_ipatlas_intervals, bench_game_snapshot);
criterion_main!(benches);
