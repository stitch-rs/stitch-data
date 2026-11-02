# RFC: Stitch Data Wire Format Wire Format Specification v1.1

- **Status**: Stable Specification
- **Version**: 1.1.0
- **Authors**: GoldSrc Systems Architecture Team
- **Target Substrates**: `#![no_std]`, `stitch-core`, `zerocopy`

---

## 1. Abstract

Stitch Data Wire Format is an adaptive, domain-aware, contract-oriented
binary serialization and framing protocol engineered for ultra-low latency,
high-throughput, and extreme entropy density across heterogeneous network
and IPC substrates.

Unlike generic serializers (Protobuf, MessagePack, Bincode) that either
sacrifice compression ratio or incur heavy CPU decompression overheads,
CBP introduces:

1. **The First Bytes Axiom**: Compile-time static framing contracts
   deterministically inferred from a 2-byte prefix (`Magic` + `Version`),
   eliminating dynamic header self-description overhead.
2. **Layered Adaptive Profiles (-O0 to -O3)**: Monomorphic optimization
   pipelines targeting distinct operational regimes (Zero-Copy IPC,
   Sub-microsecond Game Ticks, Balanced Telemetry, and Archival).
3. **Monomorphic Traversal Pipelines**: Seamless integration with the Sewing
   Machine Architecture (`stitch-core`) for zero-allocation streaming
   transforms.
4. **Zero-Copy Streaming & Vectored Transport**: Scatter-gather framing,
   SIMD-accelerated stream resynchronization, and zero-allocation cursor
   readers.
5. **Compile-Time Schema Fingerprinting**: Fail-fast ABI validation protecting
   distributed endpoints from schema drift with zero dynamic overhead.

---

## 2. The First Bytes Axiom & Frame Geometry

Every CBP v1 frame begins with a fixed 2-byte prefix followed by a compact
profile/flags byte, variable payload length, optional schema hash, the payload
itself, and a mandatory Castagnoli CRC32 integrity footer:

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Magic (0xCB) | Version (0x01)| Profile | Flags |   Reserved  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       Payload Length (1..9 Bytes, PrefixVarint Encoded)       ...
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Optional Schema Fingerprint (8 Bytes, FNV-1a 64-bit Hash)   ...
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Payload Data (0..N Bytes)                   ...
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|             Integrity Checksum (4 Bytes, CRC32-C)             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 2.1 The Axiom Invariant

Upon observing `Magic == 0xCB` and `Version == 0x01`:

- The exact bitwise geometry of the Header and Footer is **statically
  immutable**.
- The parser does not require runtime length negotiation or dynamic TLV tag
  dictionaries to parse the envelope.
- Header fields are packed contiguously without speculative padding.

### 2.2 Version 1 Header Geometry (Byte-by-Byte Breakdown)

<!-- markdownlint-disable MD013 -->
| Offset / Field | Size | Encoding | Semantics & Values |
| :--- | :--- | :--- | :--- |
| `[0]` **Magic** | 1 Byte | `0xCB` | CBP protocol identifier. |
| `[1]` **Version** | 1 Byte | `0x01` | Protocol wire specification version 1. |
| `[2]` **Profile/Flags** | 1 Byte | Bitfield | Bits 0..1: Profile, Bits 2..5: Flags, Bits 6..7: Reserved (0). |
| `[3..]` **PayloadLength** | 1..9 B | `PrefixVarint` | Uncompressed payload size in bytes. Upper bound is strictly capped at `MAX_FRAME_PAYLOAD_LEN` = 64 MiB (67,108,864 bytes); frames exceeding this bound MUST be rejected with `StitchError::FrameTooLarge`. |
| `[..]` **SchemaHash** | 0 or 8 B | Little-Endian `u64` | Present if and only if `Flags.has_schema_hash == 1`. |
<!-- markdownlint-enable MD013 -->

#### Profile & Flags Bitfield Layout (`Byte 2`)

```text
  Bit:    7   6   5   4   3   2   1   0
        +---+---+---+---+---+---+---+---+
        |  Resv |   Flags   |  Profile  |
        +---+---+---+---+---+---+---+---+
```

- **Bits 0..1 (Profile)**:
  - `00` (`0x0`): `-O0 Direct` (Zero-copy raw memory dump).
  - `01` (`0x1`): `-O1 Realtime` (Aligned PrefixVarint + SIMD bit-pack).
  - `10` (`0x2`): `-O2 Balanced` (Delta-encoding + bit-pack + RLE).
  - `11` (`0x3`): `-O3 MaxDensity` (Columnar SoA + dynamic entropy selection).
- **Bits 2..5 (Flags)**:
  - Bit 2 (`0x04`): `is_compressed` — Payload underwent compression pipeline.
  - Bit 3 (`0x08`): `is_chunked` — Multi-chunk streaming frame.
  - Bit 4 (`0x10`): `has_schema_hash` — 8-byte `SCHEMA_HASH` immediately follows
    `PayloadLength`.
  - Bit 5 (`0x20`): Reserved (must be 0).
- **Bits 6..7 (Reserved)**:
  - Must be zero. Non-zero values MUST be rejected with `StitchError::InvalidHeaderFlags`.

### 2.3 Version 1 Footer Geometry

The frame concludes with a mandatory 4-byte integrity checksum:

- **Algorithm**: Castagnoli CRC32 (polynomial `0x1EDC6F41`, reflected `0x82F63B78`).
- **Initial State**: `0xFFFFFFFF`, Final XOR: `0xFFFFFFFF`.
- **Coverage**: Computes over all bytes of `Header` (bytes 0 through start of payload)
  concatenated with `Payload`.
- **Byte Order**: 32-bit unsigned integer stored in little-endian byte order
  (`[u8; 4]`).

---

## 3. Layered Optimization Profiles (-O0 .. -O3)

CBP defines 4 operational profiles for streaming pipelines (`CompressionPipeline`),
alongside dual-profile dispatch for individual records (`Record`):

- Streaming pipelines in `cbp-transforms` execute monomorphic compile-time
  traversal across all 4 profiles (`-O0` through `-O3`).
- Single-record serialization via `Record` dispatches between `-O0 Direct`
  (zero-copy uncompressed POD byte layout) and `-O1..-O3 Compact` (branchless
  PrefixVarint, ZigZag, and sub-byte bit packing).

### 3.1 Profile Direct (`-O0`)

- **Objective**: Pure memory bus saturation, zero CPU overhead.
- **Wire Layout**: Raw endian-native byte memory dump.
- **Payload Format**: Directly deserializable into `zerocopy::FromBytes` structs.
- **Throughput**: Verified at **55.6 GiB/s**.

### 3.2 Profile Realtime (`-O1`)

- **Objective**: Sub-microsecond game network snapshots and HFT market feeds.
- **Wire Layout**:
  - Monotonic identifiers encoded via branchless `PrefixVarint`.
  - Integer coordinates bounded to narrow bit-widths using SIMD bit-packing.
- **Throughput**: Verified at **14.0 GiB/s** with sub-20 ns single-tick latency.

### 3.3 Profile Balanced (`-O2`)

- **Objective**: High-density network transport with minimal CPU taxation.
- **Wire Layout**:
  - `DeltaTransformer` on monotonic and sequential numeric fields.
  - `BitPacker` with automatic bit-width truncation.
  - `RleTransformer` on repetitive status codes and flag fields.
- **Throughput**: Verified at **3.58 GiB/s**; wire size reduced by 50–75%.

### 3.4 Profile MaxDensity (`-O3`)

- **Objective**: Approaching the Shannon entropy limit for archival storage.
- **Wire Layout**:
  - Dynamic entropy competition: selects minimum representation between
    RLE, Delta+BitPack, and Gorilla Float per block in `CompressionPipeline`.
  - True Columnar Struct-of-Arrays (SoA) transposition is scheduled for v0.3.0.
- **Throughput**: Verified at **1.84 GiB/s**; 3.97x denser than raw AoS.

---

## 4. Primitive Encoding Rules

### 4.1 Branchless `PrefixVarint`

`PrefixVarint` encodes variable-length unsigned 64-bit integers (`0..=u64::MAX`)
in 1 to 9 bytes without continuation bits or sequential loop branching:

```text
Value Range                  Length    Tag Prefix Byte (Binary)
[0, 127]                     1 Byte    0xxxxxxx
[128, 16,383]                2 Bytes   10xxxxxx + 1 Byte
[16,384, 2,097,151]          3 Bytes   110xxxxx + 2 Bytes
[2,097,152, 268,435,455]     4 Bytes   1110xxxx + 3 Bytes
[268,435,456, 34,359,738,367]5 Bytes   11110xxx + 4 Bytes
...                          ...       ...
[2^56, u64::MAX]             9 Bytes   11111111 + 8 Bytes
```

#### Decoding Invariant

The number of trailing/leading ones in the first byte dictates the exact payload
byte count. Decoding reads the full integer via a single unaligned load and
masked shift in a single CPU cycle.

### 4.2 ZigZag Signed Integer Mapping

Signed integers ($i32, i64$) are mapped to unsigned integers to project small
absolute magnitudes near zero:

$$\text{ZigZag32}(n) = (n \ll 1) \oplus (n \gg 31)$$

$$\text{ZigZag64}(n) = (n \ll 1) \oplus (n \gg 63)$$

### 4.3 Signed Integer Wrapping Delta

Signed deltas are calculated using modular two's complement arithmetic:

$$\Delta_i = v_i \mathbin{\text{wrapping\_sub}} v_{i-1}$$

$$v_i = v_{i-1} \mathbin{\text{wrapping\_add}} \Delta_i$$

Preserves full integer range $[-2^{31}, 2^{31}-1]$ without overflow panics.

### 4.4 Gorilla Floating-Point XOR Compression

Lossless IEEE-754 floating-point compression (`f32` and `f64`) adapts the
Gorilla XOR delta algorithm:

1. **Header Value**: The first float is stored uncompressed as 4 bytes (`f32`)
   or 8 bytes (`f64`) in little-endian byte order.
2. **Subsequent Values**:
   - Compute bitwise XOR: $D = \text{bits}(v_i) \oplus \text{bits}(v_{i-1})$.
   - If $D = 0$: Emit a single bit `0`.
   - If $D \neq 0$: Emit bit `1`.
     - If the number of leading zeros and trailing zeros of $D$ match the previous
       delta's bounds: Emit bit `0`, followed by $L$ meaningful bits.
     - Else: Emit bit `1`, followed by 5 bits (`f32`) or 6 bits (`f64`) of leading
       zero count, 6 bits (`f32`) or 7 bits (`f64`) of meaningful bit length $L$,
       and then $L$ meaningful bits.
3. **Decryption Invariant & Security Constraint**:
   - Decoders MUST enforce $\text{leading} + \text{length} \le 32$ (for `f32`) and
     $\text{leading} + \text{length} \le 64$ (for `f64`), and $\text{length} > 0$.
   - Any bitstream violating this invariant MUST be immediately rejected with
     `StitchError::PayloadDecodeError` to prevent left-shift overflow attacks.

### 4.5 Bounded Bit-Packing

Bit-packed blocks store homogeneous integers packed at sub-byte bit widths:

```text
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| BitWidth (1B) |               Packed Bitstream Data           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- `BitWidth` specifies the bit allocation per integer ($W \in [1, 32]$).
- A `BitWidth` of $0$ or $> 32$ MUST yield `StitchError::InvalidBitWidth`.
- Decoders employ AVX2 vector zero-extension (`_mm256_cvtepu8_epi32` for $W=8$,
  `_mm256_cvtepu16_epi32` for $W=16$) and 64-bit SWAR register accumulators
  for unaligned widths.

---

## 5. Transport, Streaming & Resynchronization

### 5.1 Zero-Copy Vectored Framing (`VectoredFrame`)

To prevent memory copying during network I/O, encoders construct a `VectoredFrame`:

- **Component 1**: Stack-allocated header buffer (`[u8; 24]`).
- **Component 2**: Borrowed payload slice (`&'a [u8]`).
- **Component 3**: Stack-allocated CRC32 footer (`[u8; 4]`).

Exposes `.slices() -> [&[u8]; 3]` and `.io_slices() -> [IoSlice; 3]` for
direct submission to scatter-gather network system calls (`writev`, `sendmsg`,
`WSASend`).

### 5.2 Streaming Cursor Reader (`FrameReader`)

`FrameReader<'a>` decodes multi-frame streams (TCP buffers, shared memory rings)
with zero allocations:

- `next_frame(&mut self) -> Result<Option<FrameView<'a>>, StitchError>`:
  - Returns `Ok(Some(FrameView))` upon encountering a complete, CRC-valid frame.
  - Advances the internal cursor by `frame_len`.
  - Returns `Ok(None)` when the buffer is cleanly exhausted.
  - Returns `Err(StitchError::UnexpectedEof)` if a partial frame remains.

### 5.3 Resilient Stream Resynchronization (`StreamResyncParser`)

On noisy or byte-corrupted transports:

1. **AVX2 Magic Word Scanning (`find_magic`)**:
   - Compares 32 bytes simultaneously against `[0xCB, 0x01]` using
     `_mm256_cmpeq_epi8` and hardware trailing-zero bit scanning at **35.4 GiB/s**.
2. **CRC Verification Gate**:
   - Candidate frames are validated against their 32-bit Castagnoli CRC. False
     positives from payload bytes resembling magic numbers are rejected.
3. **Resynchronization Statistics**:
   - Discarded noise bytes and resync events are tracked in `ResyncStats`.

---

## 6. Schema Fingerprinting & Derive Macros

### 6.1 Deterministic 64-bit FNV-1a Schema Hash

Every struct annotated with `#[derive(Record)]` generates a compile-time
canonical schema string:

<!-- markdownlint-disable MD013 -->
$$\text{SchemaString} = \text{StructName} + "\{" + \sum (\text{FieldName} + ":" + \text{FieldType} + "@" + \text{Codec} + ";") + "\}"$$
<!-- markdownlint-enable MD013 -->

The 64-bit FNV-1a hash of this UTF-8 string defines `Record::SCHEMA_HASH`.

### 6.2 Fail-Fast ABI Compatibility Contract

- If `has_schema_hash` flag is set, the frame header contains the 8-byte hash.
- Upon decoding, if `header.schema_hash != Some(Self::SCHEMA_HASH)`, the decoder
  MUST abort immediately and return:

$$\text{StitchError::SchemaMismatch}\{\text{expected}, \text{actual}\}$$

No unverified payload bytes are decoded when schemas diverge.

### 6.3 Record Batch Builder (`RecordBatchBuilder` / `ColumnarBlockBuilder`)

`RecordBatchBuilder<T: Record, const CAPACITY: usize>` (aliased as
`ColumnarBlockBuilder` for backward compatibility) buffers records in
an inline stack array `[MaybeUninit<T>; CAPACITY]` with $O(1)$ push. Upon
reaching capacity, `flush(&mut scratch, &mut dst)` serializes all records
into a framed, profile-optimized CBP packet with zero dynamic heap allocations.
(True Struct-of-Arrays (SoA) column transposition is specified for v0.3.0).

---

## 7. Security Invariants & Implementation Commandments

1. **No Silent Error Suppression**: Under no circumstances may a decoder
   silently truncate, return default values, or discard corrupted bytes unless
   explicitly handled by `StreamResyncParser`.
2. **Bounds-Check Elimination with Memory Safety**: All unsafe SIMD pointer
   arithmetic must be mathematically bounded by slice lengths and guarded by
   `// SAFETY:` documentation.
3. **Miri Cleanliness**: All `#![no_std]` core algorithms must execute under
   Miri with 0 Undefined Behavior, 0 unaligned reads, and 0 Stacked Borrows
   violations.
4. **Hardware CRC Fallback Consistency**: Software slicing-by-8 and hardware
   SSE4.2/ARM64 CRC32 MUST produce bit-identical checksums across all buffer
   lengths.
