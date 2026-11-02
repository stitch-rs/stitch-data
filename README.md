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
  - `-O0 Direct`: Zero-copy raw memory alignment for IPC and in-memory caches.
  - `-O1 Realtime`: Branchless `PrefixVarint` and SIMD bit-packing for game ticks.
  - `-O2 Balanced`: Delta-encoding, FastPFOR, and RLE for telemetry and mobile feeds.
  - `-O3 MaxDensity`: Columnar SoA, Elias-Fano, and dictionaries for archival.
- **Monomorphic Traversal Pipelines**: Seamless integration with the Sewing Machine
  Architecture (`stitch-core`) for zero-allocation streaming transforms.
- **Pure `#![no_std]`**: Built for embedded systems, kernels, and bare-metal environments.

## Workspace Topology

- **`crates/cbp-core`**: Core `#![no_std]` bitstream, branchless PrefixVarint,
  ZigZag, CRC32, and frame pack/unpack logic.
- **`crates/cbp-transforms`**: Monomorphic transform pipelines (Delta, BitPack,
  RLE) powered by `stitch-core`.
- **`crates/cbp`**: High-performance ergonomic facade.
- **`benches`**: Comprehensive benchmark suite against Protobuf, MessagePack,
  Bincode, and Postcard.

## Specifications

For complete byte-level framing invariants and bitwise geometries, see
[RFC Wire Format Specification v1.0](docs/RFC_WIRE_FORMAT_V1.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE) or
  <http://opensource.org/licenses/MIT>)

at your option.
