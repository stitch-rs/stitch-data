<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **True Columnar (SoA) Block Compression & In-Memory Transposition (v0.3.0)**:
  - Implemented `ColumnarBlockWriter` and `ColumnarBlockReader` for
    self-describing Structure-of-Arrays (SoA) containers with zero heap allocations.
  - Added specialized column encoding and decoding routines for
    `u32`/`u16`/`u8` (`AdaptiveU32`), `i32`/`i16`/`i8` (`DeltaZigZagI32`),
    and `f32` (`GorillaF32`).
  - Extended `#[derive(StitchRecord)]` to automatically synthesize chunked
    columnar transposition loops (processing in 256-element L1D cache blocks).
  - Upgraded `RecordBatchBuilder::flush` to dispatch to columnar compression
    when `OptimizationProfile::MaxDensity` is selected, and added
    `flush_columnar` and `decode_columnar_frame`.
  - Proved nearly 2x size reduction (264 bytes vs 513 bytes) on correlated
    sensor telemetry batches with 100% exact fidelity and 0 undefined behavior
    under Miri.

- **Specification, CI Gates & Production Examples (Category 6)**:
  - Formally upgraded wire specification to **RFC Wire Format Specification v1.1**
    in `docs/RFC_WIRE_FORMAT_V1.md` detailing Gorilla Float bitstream bounds,
    signed wrapping deltas, bounded bit-packing, zero-copy vectored framing,
    stream resynchronization, and FNV-1a 64-bit schema fingerprinting.
  - Production reference example: Real-time UDP Multiplayer Game Telemetry
    (`examples/udp_game_telemetry.rs`) simulating 64 players at 128Hz with
    `RecordBatchBuilder`, vectored framing, and stream noise recovery.
  - Production reference example: Constrained Embedded IoT / Sensor Node
    (`examples/embedded_sensor_node.rs`) demonstrating stack-only 128-byte
    encoding with zero heap allocations and schema integrity validation.
  - CI regression gates in `.github/workflows/ci.yml` running adversarial
    fuzzing (100k iterations), benchmark smoke checks, and example verification.
- **Adversarial Fuzzing, Miri UB Audit & Security Hardening (Category 5)**:
  - Adversarial fuzzing crate `crates/cbp-fuzz` and `fuzz/` libFuzzer targets
    for `unpack_frame`, `decode_prefix_varint`, `BitPacker`, `GorillaCompressor`,
    and `StreamResyncParser`.
  - Deterministic 100,000-iteration adversarial test suite covering payload
    truncation attacks across all byte boundaries, single-bit and burst CRC
    corruption rejection, DoS payload size defense ($2^{30}$ bytes), schema
    spoofing, and stream noise recovery.
  - Undefined behavior & memory safety audit with Miri (`cargo +nightly miri`):
    100% of unit and integration tests verified clean across `cbp-core`,
    `cbp-transforms`, and `cbp`, proving zero pointer aliasing violations,
    strict alignment compliance, and sound `MaybeUninit` usage.
  - Dependency vulnerability scan via `cargo audit`: 0 security CVEs found.
- **Developer Experience, Codec Layer & Macros (Category 4)**:
  - Procedural derive macro `#[derive(CbpRecord)]` in new workspace member
    `crates/cbp-derive`.
  - Field routing attributes (`#[cbp(varint)]`, `#[cbp(zigzag)]`,
    `#[cbp(raw)]`, `#[cbp(bits = N)]`, `#[cbp(codec = ...)]`) with sub-byte
    bitstream packing and dual-profile dispatch (-O0 Direct vs -O2 Balanced).
  - Compile-time 64-bit schema fingerprinting (`SCHEMA_HASH: u64`) using FNV-1a
    with fail-fast validation (`CbpError::SchemaMismatch`).
  - Cache-conscious `RecordBatchBuilder<T: CbpRecord, const CAPACITY: usize>`
    (aliased as `ColumnarBlockBuilder`) in `crates/cbp-transforms/src/columnar.rs`
    providing inline stack batching with $O(1)$ push and zero-allocation flushing.
  - Benchmarked performance: single-record encode in **8.71 ns** (**2.57 GiB/s**,
    2.19x faster than Protobuf prost and 4.92x faster than Bincode); batch 64
    snapshots in **726.69 ns** (2.08x faster than Protobuf prost) with
    **31.1% smaller wire size** and 0 heap allocations.
- **Zero-Copy, Vectored I/O & Streaming Transport (Category 3)**:
  - Incremental and multi-slice Castagnoli CRC32 (`crc32_castagnoli_update`,
    `crc32_castagnoli_slices`) across non-contiguous memory slices.
  - Scatter-gather vectored framing (`pack_frame_vectored`) with stack header,
    borrowed payload, and stack CRC footer (`VectoredFrame`), eliminating payload
    copies and providing `.io_slices()` for direct `writev` system calls.
    Accelerates 1 KiB framing by **1.82x** and saves 38.2 µs per megabyte.
  - Zero-copy cursor-based `FrameReader` and `FrameView` yielding frames from
    multi-frame stream buffers at **12.76 GiB/s** (19.34 ns/frame).
  - High-throughput stream boundary detection (`find_magic`) using AVX2 SIMD
    vector scanning at **35.41 GiB/s** (3.38x faster than SWAR).
  - Resilient stream recovery parser (`resync_frame`, `StreamResyncParser`)
    recovering valid frame boundaries from noisy or corrupted streams in
    **27.14 ns**.
- **Hardware Optimizations & SIMD Acceleration (Category 2)**:
  - Runtime AVX2 CPU feature detection (`has_avx2()`) via `__cpuid_count(7, 0)`
    and `_xgetbv(0)` for `#![no_std]`.
  - In-place vector Delta encoding (`delta_encode_u32` / `delta_encode_i32`) via
    register shifts (`_mm_slli_si128` + `_mm_sub_epi32`), achieving **36.49 GiB/s**.
  - Vectorized prefix-sum Delta decoding (`delta_decode_u32` / `delta_decode_i32`)
    using parallel Kogge-Stone scan cascade, achieving **13.27 GiB/s**.
  - High-performance SIMD & SWAR `BitPacker`:
    - 8-bit AVX2 zero-extension unpacking: **83.94 GiB/s** (53.2x speedup).
    - 16-bit AVX2 zero-extension unpacking: **69.80 GiB/s** (93.1x speedup).
    - 32-bit zero-copy word unpacking: **102.09 GiB/s** (25.0x speedup).
    - 12-bit pairwise 24-bit pack/unpack: **6.88 GiB/s** (8.8x speedup).
    - 4-bit dual-nibble pack/unpack: **26.80 GiB/s** unpack (16.5x speedup).
    - 1-bit bitmap pack/unpack: **6.24 GiB/s** unpack (3.8x speedup).
    - 64-bit SWAR accumulator bitstream reader/writer for $W \in [1, 32]$.
- **Adaptive Optimization Profiles & Transforms (Category 1)**:
  - Dynamic behavioral profile dispatch in monomorphic pipelines:
    - `-O0 Direct`: Zero-overhead memory bus saturation (**55.6 GiB/s**).
    - `-O1 Realtime`: Aligned sub-microsecond game tick codec (**14.0 GiB/s**).
    - `-O2 Balanced`: Monomorphic Delta + BitPack pipeline (**3.58 GiB/s**).
    - `-O3 MaxDensity`: Dynamic entropy competition (**1.84 GiB/s**).
  - Lossless Floating-Point Compression (`GorillaCompressor` in `float.rs`):
    - IEEE-754 XOR delta encoding achieving **58.2% wire saving on f32** and
      **64.7% wire saving on f64** with bit-exact preservation.
  - Signed Integer Delta Pipeline:
    - In-place wrapping deltas for `i32` (`delta_encode_i32`, `delta_decode_i32`)
      with two's complement safety.
- **Hardware-Accelerated Castagnoli CRC32 (P0 Milestone)**:
  - Architecture-specific hardware acceleration: Intel x86_64 SSE4.2
    (`_mm_crc32_u64` unroll x4) and ARMv8 CRC (`__crc32cd`).
  - Compile-time slicing-by-8 fallback table (8 KiB in `.rodata`) for `#![no_std]`.
  - Achieved **11.00 GiB/s** CRC32 throughput (46.1x speedup) and **6.76 µs**
    `pack_frame` at 64 KiB (38.8x speedup).
- Multi-target Criterion microbenchmark harness (`benches/benches/*.rs`).
- Deterministic size and roundtrip verification harness (`benches/src/main.rs`)
  with global `CountingAllocator` verifying zero heap allocations on hot paths.
- Deterministic Lehmer LCG workload datasets and bench-only reference containers
  in `benches/src/scenarios.rs`.
- Comprehensive empirical performance analysis report and claims audit in
  `docs/BENCHMARKS.md` (IPAtlas rigor level).
- Smoke benchmark and invariant check step in `.github/workflows/ci.yml`.

### Fixed

- **Gorilla Float Bitstream Bounds Validation**: Fixed integer underflow and
  left-shift overflow in `GorillaCompressor::decompress_f32` and `decompress_f64`
  by rejecting malformed bitstreams with `leading + len > 32` or
  `leading + len > 64` with `CbpError::PayloadDecodeError` (discovered via
  adversarial fuzzer).
- **Issue CBP-01**: Fixed off-by-one return in `BitPacker::unpack_u32` by
  implementing `BitReader::bytes_read()`, ensuring exact bitstream alignment.

## [0.1.0] - 2025-10-04

### Added

- Initial project structure (legacy prototype).
