# Compact Binary Protocol (CBP)

[![CI](https://github.com/ulquiorracode/compact-binary-protocol/actions/workflows/ci.yml/badge.svg)](https://github.com/ulquiorracode/compact-binary-protocol/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust: 2024](https://img.shields.io/badge/Language-Rust%202024-orange.svg)](https://www.rust-lang.org/)

Compact Binary Protocol (CBP) is an adaptive, domain-aware, contract-oriented
binary serialization and framing engine in Rust engineered for ultra-low latency,
high-throughput, and extreme entropy density.

---

## Key Pillars

- **The First Bytes Axiom**: Compile-time static framing geometry inferred from
  a 2-byte prefix (`0xCB` Magic + `0x01` Version), eliminating self-describing
  metadata bloat.
- **Layered Optimization Profiles (-O0 to -O3)**:
  - `-O0 Direct`: Zero-copy raw memory alignment for IPC and in-memory caches
    (55.6 GiB/s).
  - `-O1 Realtime`: Branchless `PrefixVarint` and compact bit-packing for game
    ticks (14.0 GiB/s).
  - `-O2 Balanced`: Delta-encoding and RLE for telemetry and mobile feeds
    (3.58 GiB/s).
  - `-O3 MaxDensity`: Columnar SoA, entropy-competition transforms for archival
    (1.84 GiB/s).
- **Monomorphic Traversal Pipelines**: Seamless integration with the Sewing Machine
  Architecture (`stitch-core`) for zero-allocation streaming transforms.
- **Hardware-Accelerated Mechanical Sympathy**:
  - SSE4.2 / ARMv8 hardware Castagnoli CRC32 (11.0 GiB/s).
  - AVX2 SIMD zero-extension bit-unpacking (up to 102 GiB/s) and vector Delta
    encoding (36.5 GiB/s).
- **Zero-Copy & Vectored Streaming Transport**:
  - Scatter-gather vectored framing (`pack_frame_vectored`) eliminating payload
    copies for `writev` / socket dispatch (1.8x faster on 1 KiB, saves 38 µs/MB).
  - Zero-copy `FrameReader` for continuous stream decoding (12.8 GiB/s).
  - Fast-forward stream resynchronization (`find_magic` at 35.4 GiB/s, recovers
    from noise in 27 ns).
- **Zero-Cost Derive Macros & Schema Fingerprinting**:
  - `#[derive(CbpRecord)]` with field routing attributes (`#[cbp(delta)]`,
    `#[cbp(varint)]`, `#[cbp(zigzag)]`, `#[cbp(gorilla)]`, `#[cbp(raw)]`,
    `#[cbp(bits = N)]`).
  - 64-bit compile-time schema fingerprinting (`SCHEMA_HASH: u64`) and fail-fast
    validation (`CbpError::SchemaMismatch`).
  - `ColumnarBlockBuilder<T, CAPACITY>` for stack-allocated batching with zero
    heap allocations (encode in 8.71 ns, 2.19x faster than Protobuf).
- **Pure `#![no_std]`**: Built for embedded systems, kernels, and bare-metal
  environments.

## Workspace Topology

- **`crates/cbp-core`**: Core `#![no_std]` bitstream, branchless PrefixVarint,
  ZigZag, hardware-accelerated Castagnoli CRC32 (SSE4.2/ARM64), and frame
  pack/unpack logic.
- **`crates/cbp-transforms`**: Monomorphic transform pipelines (Delta, BitPack,
  RLE, Gorilla Float) with AVX2 SIMD acceleration powered by `stitch-core`, and
  `ColumnarBlockBuilder`.
- **`crates/cbp-derive`**: Procedural derive macro `#[derive(CbpRecord)]` with
  field attribute routing and compile-time schema hashing.
- **`crates/cbp`**: High-performance ergonomic facade.
- **`crates/cbp-fuzz`**: Deterministic 100,000-iteration adversarial fuzzing test
  suite and targets.
- **`fuzz`**: Native `cargo-fuzz` / `libFuzzer` target harnesses.
- **`benches`**: Comprehensive benchmark suite against Protobuf, MessagePack,
  Bincode, and Postcard.

## Quickstart

```rust
use cbp::prelude::*;

#[derive(CbpRecord, Debug, PartialEq, Clone, Copy)]
pub struct PlayerSnapshot {
    #[cbp(varint)]
    pub player_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub health: u16,
    pub flags: u16,
    #[cbp(varint)]
    pub tick: u32,
}

fn main() -> Result<(), CbpError> {
    let snap = PlayerSnapshot {
        player_id: 1042,
        x: 12.5,
        y: 64.0,
        z: -120.0,
        health: 100,
        flags: 0x01,
        tick: 58291,
    };

    // 1. Zero-allocation single record encoding (8.71 ns)
    let mut raw_buf = [0u8; 64];
    let raw_len = snap.encode_record(&mut raw_buf)?;

    // 2. Framed packing with 64-bit schema hash & hardware CRC32 (18.43 ns)
    let mut scratch = [0u8; 64];
    let mut frame_buf = [0u8; 64];
    let frame_len = snap.pack_to_frame(
        OptimizationProfile::Balanced,
        &mut scratch,
        &mut frame_buf,
    )?;

    // 3. Schema-verified unpacking with instant fail-fast on schema mismatch
    let decoded = PlayerSnapshot::unpack_from_frame(&frame_buf[..frame_len])?;
    assert_eq!(snap, decoded);
    Ok(())
}
```

## Performance & Empirical Density

CBP is benchmarked under strict empirical controls against leading binary formats
on dedicated bare-metal hardware (Intel Core i9-11900H, Windows 11).

### Wire Size Scoreboard Summary

<!-- markdownlint-disable MD013 -->
| Workload Scenario | Raw AoS | Bincode | Postcard | Protobuf (`prost`) | MessagePack | CBP Engine |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1,024 IP Routing Intervals (12 KB raw)** | 12,288 B | 12,296 B | 9,218 B | 9,225 B | 12,291 B | **3,098 B (3.97x denser, 33.6% of Protobuf)** |
| **Single Game Tick Snapshot (24B raw)** | 24 B | 24 B | 19 B | 26 B | 26 B | **27 B (Includes 32-bit CRC32 checksum)** |
| **Batch 64 Game Snapshots (1,536B raw)** | 1,536 B | 1,544 B | 1,242 B | 1,815 B | 1,668 B | **1,251 B (Includes 32-bit CRC32 checksum)** |
<!-- markdownlint-enable MD013 -->

For the complete benchmark suite, latency/throughput breakdowns, cache scaling
analysis, counting allocator zero-heap verification, and claims audit, see
[Microbenchmark & Empirical Analysis](docs/BENCHMARKS.md).

## Production Reference Examples

Runnable reference implementations demonstrating real-world integration:

- **Real-Time UDP Game Telemetry**: Run
  `cargo run -p cbp --example udp_game_telemetry` (64 players @ 128Hz,
  columnar batching, and stream noise recovery).
- **Constrained Embedded IoT Sensor Node**: Run
  `cargo run -p cbp --example embedded_sensor_node` (stack-only 128-byte budget,
  zero dynamic heap allocations, `#![no_std]`).

## Specifications

For complete byte-level framing invariants and bitwise geometries, see
[RFC Wire Format Specification v1.1](docs/RFC_WIRE_FORMAT_V1.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE) or
  <http://opensource.org/licenses/MIT>)

at your option.
