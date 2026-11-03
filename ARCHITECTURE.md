# Stitch Data Engine (`stitch-data`) Technical Reference

**Specification Identifier:** `ARCH-SPEC-STITCH-DATA-001`  
**Classification:** Normative Data Layout & Columnar Invariant Contracts  
**Status:** Canonical (Single Source of Truth)  

---

## 1. Normative Authority & Contract Hierarchy

This document serves as the **sole normative technical specification** for the `stitch-data` serialization and columnar transformation engine.

- `ARCHITECTURE.md`: The authoritative reference for memory layouts, binary container framing, bitstream encodings, SIMD acceleration, and cache-conscious SoA structures.
- `README.md`: Informative landing page, getting-started examples, and high-level benchmarks. In any discrepancy, `ARCHITECTURE.md` supersedes `README.md`.

---

## 2. Core Architectural Axioms

1. **Zero Dynamic Allocation (`#![no_std]`)**:
   - All compression pipelines, bitpackers, columnar builders, and frame packers operate strictly over pre-allocated caller memory, stack scratchpads, or slice views.
   - Core computational routines require 0 bytes of heap allocation.

2. **The Scrooge Systems Mindset (Mechanical Sympathy)**:
   - Data structures align to 64-byte L1D cache boundaries.
   - Monomorphic execution via Sewing Machine Architecture (`stitch-core`) eliminates indirect vtable lookups on hot transformation paths.
   - Bitpacking and delta decoding utilize hardware SIMD (AVX2 on x86_64, NEON on aarch64) with pure SWAR (SIMD Within A Register) fallbacks.

3. **Lossless and Deterministic Invariants**:
   - By default, all profiles (`Direct`, `Realtime`, `Balanced`, `MaxDensity`) execute strictly lossless transformations.
   - Float compression utilizes lossless IEEE-754 XOR delta compression (Gorilla algorithm) preserving NaN bit-patterns and signed zeroes.

---

## 3. Physical Layout & Framing Format

Every `stitch-data` packet complies with canonical framing:

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Magic (CB)  | Version (0x01)|  Profile Tag  |  Flag Bits    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       Prefix Varint Encoded Payload Length (1 - 9 bytes)      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| [Optional 8-Byte Schema FNV-1a Hash if FLAG_SCHEMA_HASH set]  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Compressed / Raw Payload                  |
|                              ...                              |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|              Hardware Castagnoli CRC-32C Checksum             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 3.1 Header Fields & Alignment

- `Magic`: 1 byte (`0xCB`), identifying CBP frame boundary.
- `Version`: 1 byte (`0x01`).
- `Profile`: 1 byte (`0x00`: Direct, `0x01`: Realtime, `0x02`: Balanced, `0x03`: MaxDensity).
- `Flags`: 1 byte bitmask (`is_compressed`, `is_chunked`, `has_schema_hash`).
- `Payload Length`: 1..9 bytes prefix varint (zero-allocation branchless decoding).
- `CRC-32C`: 4 bytes IEEE 802.3 / iSCSI Castagnoli CRC-32C hardware polynomial (`0x1EDC6F41`), calculated over header + payload via 16-way slicing-by-8/16.

---

## 4. Columnar (SoA) Encoding & $O(1)$ Slicing

For columnar record batches (`RecordBatchBuilder`), the internal payload transitions from Array-of-Structs (AoS) to Structure-of-Arrays (SoA) layout:

```text
+-----------------------+-----------------------+-----------------------+
|  Row Count (Varint)   | Column Count (Varint) |  Column 0 Descriptor  |
+-----------------------+-----------------------+-----------------------+
|   Column 0 Payload    |  Column 1 Descriptor  |   Column 1 Payload    |
+-----------------------+-----------------------+-----------------------+
```

### 4.1 Column Codec Tags (`ColumnCodecTag`)

- `0x00` (`Raw`): Uncompressed POD bytes, enabling direct $O(1)$ pointer casting via `read_raw_at<T>`.
- `0x01` (`AdaptiveU32`): Delta + BitPacking or RLE based on empirical minimum footprint.
- `0x02` (`DeltaZigZagI32`): ZigZag projection mapped to unsigned monotonic deltas + BitPacking.
- `0x03` (`GorillaF32`): IEEE-754 XOR floating point delta bitstream.
- `0x04` (`Rle`): Run-Length Encoding for repetitive runs.
- `0x05` (`Dictionary`): Categorical integer dictionary for high cardinality symbols.

### 4.2 In-Place Random-Access Slicing (`ColumnSliceIndex`)

Rather than decompressing full columns sequentially:

- `ColumnSliceIndex<'a, MAX_COLUMNS>` indexes column slice boundaries in $O(K)$ time (where $K$ is column count $\le 16$).
- `read_raw_at<T>(col_idx, row_idx)` provides zero-copy $O(1)$ random-access cell reads.
- `read_bitpacked_u32_at(payload, row_idx, total_count)` provides branchless $O(1)$ bit extraction directly from packed bitstreams without decompression.

---

## 5. Concurrency & Re-Entrancy Contracts

1. **Thread-Safety**: All transformations operate on borrowed slices (`&[u8]` and `&mut [u8]`), ensuring `Send + Sync` compliance through standard Rust borrow-checker invariants.
2. **Deterministic Reproducibility**: Given identical input buffers, compression and framing output is bit-for-bit identical across all CPU architectures (x86_64, aarch64, wasm32).
