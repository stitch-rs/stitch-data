//! Benchmark scenarios, deterministic datasets, and reference serializers.
//!
//! Provides deterministic pseudo-random workload generators (Lehmer LCG),
//! standard test domain models, and bench-only reference container implementations.

use cbp::prelude::*;
use prost::Message;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Deterministic Pseudo-Random Generator (Lehmer LCG, 0 external deps)
// ============================================================================

/// Minimal 64-bit Lehmer / Knuth LCG pseudo-random generator with zero heap dependencies.
#[derive(Debug, Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// Creates a new LCG initialized with `seed`.
    #[inline]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x853c49e6748fea9b } else { seed },
        }
    }

    /// Generates next pseudo-random `u32`.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }

    /// Generates next pseudo-random `u32` within `[min, max)`.
    #[inline]
    pub fn next_u32_range(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        min + (self.next_u32() % (max - min))
    }

    /// Generates next pseudo-random `f32` in `[0.0, 1.0)`.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        let mantissa = self.next_u32() & 0x00FF_FFFF;
        (mantissa as f32) / (0x0100_0000 as f32)
    }
}

// ============================================================================
// 2. Domain Models (AoS & Protobuf definitions)
// ============================================================================

/// Scenario A: IPAtlas Range Interval (12 bytes raw AoS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, CbpRecord)]
#[repr(C)]
pub struct Ipv4Interval {
    #[cbp(varint)]
    pub ip_from: u32,
    #[cbp(varint)]
    pub ip_to: u32,
    #[cbp(bits = 6)]
    pub profile_id: u32,
}

/// Scenario A Protobuf Container.
#[derive(Clone, PartialEq, Message)]
pub struct ProtoIpv4IntervalList {
    #[prost(uint32, repeated, tag = "1")]
    pub ip_from: Vec<u32>,
    #[prost(uint32, repeated, tag = "2")]
    pub ip_to: Vec<u32>,
    #[prost(uint32, repeated, tag = "3")]
    pub profile_id: Vec<u32>,
}

/// Scenario B: Multiplayer Real-Time Game Telemetry Snapshot (24 bytes raw AoS).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, CbpRecord)]
#[repr(C)]
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

/// Scenario B Protobuf Snapshot.
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

/// Scenario B Protobuf Snapshot Batch.
#[derive(Clone, PartialEq, Message)]
pub struct ProtoPlayerSnapshotBatch {
    #[prost(message, repeated, tag = "1")]
    pub snapshots: Vec<ProtoPlayerSnapshot>,
}

// ============================================================================
// 3. Workload Data Generators
// ============================================================================

/// Generates a deterministic sequence of `count` monotonic IP intervals.
/// Simulates global BGP / IP database routing table segments.
pub fn generate_ipatlas_intervals(count: usize) -> Vec<Ipv4Interval> {
    let mut intervals = Vec::with_capacity(count);
    let mut current_ip = 16_777_216u32; // 1.0.0.0
    let mut lcg = Lcg::new(0x1337_C0DE_A5A5_5A5A);

    for _ in 0..count {
        let span = lcg.next_u32_range(16, 256);
        let gap = lcg.next_u32_range(1, 8);
        let profile = lcg.next_u32_range(0, 64);

        intervals.push(Ipv4Interval {
            ip_from: current_ip,
            ip_to: current_ip.wrapping_add(span),
            profile_id: profile,
        });

        current_ip = current_ip.wrapping_add(span + gap);
    }

    intervals
}

/// Converts `&[Ipv4Interval]` to Protobuf repeated format.
pub fn intervals_to_proto(intervals: &[Ipv4Interval]) -> ProtoIpv4IntervalList {
    let mut ip_from = Vec::with_capacity(intervals.len());
    let mut ip_to = Vec::with_capacity(intervals.len());
    let mut profile_id = Vec::with_capacity(intervals.len());

    for item in intervals {
        ip_from.push(item.ip_from);
        ip_to.push(item.ip_to);
        profile_id.push(item.profile_id);
    }

    ProtoIpv4IntervalList {
        ip_from,
        ip_to,
        profile_id,
    }
}

/// Generates a single deterministic game player snapshot.
pub fn generate_player_snapshot(seed: u64) -> PlayerSnapshot {
    let mut lcg = Lcg::new(seed);
    PlayerSnapshot {
        player_id: lcg.next_u32_range(1000, 2000),
        x: lcg.next_f32() * 4096.0 - 2048.0,
        y: lcg.next_f32() * 4096.0 - 2048.0,
        z: lcg.next_f32() * 512.0,
        health: lcg.next_u32_range(0, 101) as u16,
        flags: lcg.next_u32_range(0, 256) as u16,
        tick: lcg.next_u32_range(100_000, 200_000),
    }
}

/// Generates a deterministic batch of `count` player snapshots.
pub fn generate_player_snapshots(count: usize) -> Vec<PlayerSnapshot> {
    let mut result = Vec::with_capacity(count);
    let mut lcg = Lcg::new(0xBADC_0DED_1234_5678);
    for _ in 0..count {
        result.push(PlayerSnapshot {
            player_id: lcg.next_u32_range(1, 1000),
            x: lcg.next_f32() * 8192.0 - 4096.0,
            y: lcg.next_f32() * 8192.0 - 4096.0,
            z: lcg.next_f32() * 1024.0,
            health: lcg.next_u32_range(0, 101) as u16,
            flags: lcg.next_u32_range(0, 256) as u16,
            tick: lcg.next_u32_range(128_000, 129_000),
        });
    }
    result
}

/// Converts `&[PlayerSnapshot]` to Protobuf batch format.
pub fn snapshots_to_proto(snapshots: &[PlayerSnapshot]) -> ProtoPlayerSnapshotBatch {
    let list = snapshots
        .iter()
        .map(|s| ProtoPlayerSnapshot {
            player_id: s.player_id,
            x: s.x,
            y: s.y,
            z: s.z,
            health: s.health as u32,
            flags: s.flags as u32,
            tick: s.tick,
        })
        .collect();
    ProtoPlayerSnapshotBatch { snapshots: list }
}

/// Generates a monotonic `u32` sequence with a constant step.
pub fn generate_monotone_u32(count: usize, base: u32, step: u32) -> Vec<u32> {
    let mut res = Vec::with_capacity(count);
    let mut cur = base;
    for _ in 0..count {
        res.push(cur);
        cur = cur.wrapping_add(step);
    }
    res
}

/// Generates a pseudo-random `u32` sequence bounded to `max_val`.
pub fn generate_bounded_u32(count: usize, max_val: u32, seed: u64) -> Vec<u32> {
    let mut lcg = Lcg::new(seed);
    let mut res = Vec::with_capacity(count);
    for _ in 0..count {
        if max_val == u32::MAX {
            res.push(lcg.next_u32());
        } else {
            res.push(lcg.next_u32() % (max_val + 1));
        }
    }
    res
}

/// Generates an RLE pattern with runs of approximately `run_len`.
pub fn generate_rle_u32(count: usize, run_len: usize, seed: u64) -> Vec<u32> {
    let mut lcg = Lcg::new(seed);
    let mut res = Vec::with_capacity(count);
    while res.len() < count {
        let val = lcg.next_u32() % 1000;
        let actual_run = core::cmp::min(run_len, count - res.len());
        for _ in 0..actual_run {
            res.push(val);
        }
    }
    res
}

// ============================================================================
// 4. Bench-Only Reference Containers (Honest Full-Record Serialization)
// ============================================================================

/// Bench-only reference columnar container for `[Ipv4Interval]`.
///
/// Encodes all 3 columns (`ip_from`, `ip_to`, `profile_id`) using CBP
/// monomorphic pipelines and packs into a validated CBP Frame with Castagnoli CRC32.
///
/// NOTE: This is NOT a public generic serializer; it is a reference implementation
/// modeling how an application schema maps onto CBP primitives and transforms.
pub struct CbpReferenceIntervalContainer;

impl CbpReferenceIntervalContainer {
    /// Encodes a full slice of `Ipv4Interval` into `frame_out`.
    /// `scratch_u32` must have capacity at least `intervals.len() * 2`.
    /// Zero heap allocations when caller pre-allocates scratch buffers.
    pub fn encode(
        intervals: &[Ipv4Interval],
        scratch_u32: &mut [u32],
        payload_buf: &mut [u8],
        frame_out: &mut [u8],
    ) -> Result<usize, CbpError> {
        let count = intervals.len();
        if scratch_u32.len() < count * 2 {
            return Err(CbpError::UnexpectedEof {
                required: count * 2,
                available: scratch_u32.len(),
            });
        }

        let (col_buf, pipe_scratch) = scratch_u32.split_at_mut(count);
        let mut payload_offset = 0;

        // Header: element count as PrefixVarint (needs up to 9 bytes)
        let count_varint_len = encode_prefix_varint(
            count as u64,
            &mut payload_buf[payload_offset..payload_offset + 9],
        );
        payload_offset += count_varint_len;

        // --- Column 1: ip_from (Monotonic -> Delta + BitPack) ---
        for (i, item) in intervals.iter().enumerate() {
            col_buf[i] = item.ip_from;
        }
        let mut pipeline = CompressionPipeline::new();
        let ip_from_stream_len = pipeline.compress_u32(
            &col_buf[..count],
            &mut pipe_scratch[..count],
            &mut payload_buf[payload_offset + 9..],
        )?;
        let ip_from_varint = encode_prefix_varint(
            ip_from_stream_len as u64,
            &mut payload_buf[payload_offset..payload_offset + 9],
        );
        // Slide compressed stream adjacent to its varint header if needed
        if ip_from_varint < 9 {
            payload_buf.copy_within(
                payload_offset + 9..payload_offset + 9 + ip_from_stream_len,
                payload_offset + ip_from_varint,
            );
        }
        payload_offset += ip_from_varint + ip_from_stream_len;

        // --- Column 2: ip_to (Monotonic -> Delta + BitPack) ---
        for (i, item) in intervals.iter().enumerate() {
            col_buf[i] = item.ip_to;
        }
        let mut pipeline2 = CompressionPipeline::new();
        let ip_to_stream_len = pipeline2.compress_u32(
            &col_buf[..count],
            &mut pipe_scratch[..count],
            &mut payload_buf[payload_offset + 9..],
        )?;
        let ip_to_varint = encode_prefix_varint(
            ip_to_stream_len as u64,
            &mut payload_buf[payload_offset..payload_offset + 9],
        );
        if ip_to_varint < 9 {
            payload_buf.copy_within(
                payload_offset + 9..payload_offset + 9 + ip_to_stream_len,
                payload_offset + ip_to_varint,
            );
        }
        payload_offset += ip_to_varint + ip_to_stream_len;

        // --- Column 3: profile_id (Bounded BitPack) ---
        for (i, item) in intervals.iter().enumerate() {
            col_buf[i] = item.profile_id;
        }
        let prof_stream_len =
            BitPacker::pack_u32(&col_buf[..count], &mut payload_buf[payload_offset + 9..])?;
        let prof_varint = encode_prefix_varint(
            prof_stream_len as u64,
            &mut payload_buf[payload_offset..payload_offset + 9],
        );
        if prof_varint < 9 {
            payload_buf.copy_within(
                payload_offset + 9..payload_offset + 9 + prof_stream_len,
                payload_offset + prof_varint,
            );
        }
        payload_offset += prof_varint + prof_stream_len;

        // Wrap payload in CBP Frame (-O2 Balanced profile)
        let header = FrameHeader {
            profile: OptimizationProfile::Balanced,
            flags: FrameFlags {
                is_compressed: true,
                is_chunked: true,
                has_schema_hash: false,
            },
            payload_len: payload_offset,
            schema_hash: None,
        };

        pack_frame(&header, &payload_buf[..payload_offset], frame_out)
    }

    /// Decodes a full CBP frame back into `dst: &mut [Ipv4Interval]`.
    /// Zero heap allocations when caller pre-allocates scratch buffers.
    pub fn decode(
        frame_buf: &[u8],
        scratch_col1: &mut [u32],
        scratch_col2: &mut [u32],
        scratch_col3: &mut [u32],
        dst: &mut [Ipv4Interval],
    ) -> Result<usize, CbpError> {
        let (_header, payload) = unpack_frame(frame_buf)?;

        let mut offset = 0;
        let (count_u64, varint_len) = decode_prefix_varint(&payload[offset..])?;
        let count = count_u64 as usize;
        offset += varint_len;

        if dst.len() < count
            || scratch_col1.len() < count
            || scratch_col2.len() < count
            || scratch_col3.len() < count
        {
            return Err(CbpError::PayloadDecodeError);
        }

        // --- Column 1: ip_from ---
        let (len1, var1) = decode_prefix_varint(&payload[offset..])?;
        offset += var1;
        let stream1 = &payload[offset..offset + len1 as usize];
        offset += len1 as usize;
        DecompressionPipeline::decompress_u32(stream1, &mut scratch_col1[..count])?;

        // --- Column 2: ip_to ---
        let (len2, var2) = decode_prefix_varint(&payload[offset..])?;
        offset += var2;
        let stream2 = &payload[offset..offset + len2 as usize];
        offset += len2 as usize;
        DecompressionPipeline::decompress_u32(stream2, &mut scratch_col2[..count])?;

        // --- Column 3: profile_id ---
        let (len3, var3) = decode_prefix_varint(&payload[offset..])?;
        offset += var3;
        let stream3 = &payload[offset..offset + len3 as usize];
        BitPacker::unpack_u32(stream3, &mut scratch_col3[..count])?;

        // Assemble AoS records
        for i in 0..count {
            dst[i] = Ipv4Interval {
                ip_from: scratch_col1[i],
                ip_to: scratch_col2[i],
                profile_id: scratch_col3[i],
            };
        }

        Ok(count)
    }
}

/// Bench-only reference field serializer for single `PlayerSnapshot`.
///
/// Encodes each field using PrefixVarint and IEEE-754 bit-preservation,
/// encapsulated in a validated CBP Frame with Castagnoli CRC32.
pub struct CbpReferenceSnapshot;

impl CbpReferenceSnapshot {
    /// Encodes a single player snapshot into `out_frame`.
    pub fn encode(snapshot: &PlayerSnapshot, out_frame: &mut [u8]) -> Result<usize, CbpError> {
        let mut payload = [0u8; 64];
        let mut offset = 0;

        offset += encode_prefix_varint(snapshot.player_id as u64, &mut payload[offset..offset + 9]);

        payload[offset..offset + 4].copy_from_slice(&snapshot.x.to_bits().to_le_bytes());
        offset += 4;
        payload[offset..offset + 4].copy_from_slice(&snapshot.y.to_bits().to_le_bytes());
        offset += 4;
        payload[offset..offset + 4].copy_from_slice(&snapshot.z.to_bits().to_le_bytes());
        offset += 4;

        offset += encode_prefix_varint(snapshot.health as u64, &mut payload[offset..offset + 9]);
        offset += encode_prefix_varint(snapshot.flags as u64, &mut payload[offset..offset + 9]);
        offset += encode_prefix_varint(snapshot.tick as u64, &mut payload[offset..offset + 9]);

        let header = FrameHeader {
            profile: OptimizationProfile::Realtime,
            flags: FrameFlags {
                is_compressed: false,
                is_chunked: false,
                has_schema_hash: false,
            },
            payload_len: offset,
            schema_hash: None,
        };

        pack_frame(&header, &payload[..offset], out_frame)
    }

    /// Decodes a single player snapshot from `frame_buf`.
    pub fn decode(frame_buf: &[u8]) -> Result<PlayerSnapshot, CbpError> {
        let (_header, payload) = unpack_frame(frame_buf)?;

        let mut offset = 0;
        let (player_id, v1) = decode_prefix_varint(&payload[offset..])?;
        offset += v1;

        let x = f32::from_bits(u32::from_le_bytes(
            payload[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;
        let y = f32::from_bits(u32::from_le_bytes(
            payload[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;
        let z = f32::from_bits(u32::from_le_bytes(
            payload[offset..offset + 4].try_into().unwrap(),
        ));
        offset += 4;

        let (health, v2) = decode_prefix_varint(&payload[offset..])?;
        offset += v2;
        let (flags, v3) = decode_prefix_varint(&payload[offset..])?;
        offset += v3;
        let (tick, _) = decode_prefix_varint(&payload[offset..])?;

        Ok(PlayerSnapshot {
            player_id: player_id as u32,
            x,
            y,
            z,
            health: health as u16,
            flags: flags as u16,
            tick: tick as u32,
        })
    }
}

/// Bench-only reference container for a batch of `PlayerSnapshot`s.
pub struct CbpReferenceSnapshotBatch;

impl CbpReferenceSnapshotBatch {
    /// Encodes a slice of `PlayerSnapshot` into `out_frame`.
    pub fn encode(snapshots: &[PlayerSnapshot], out_frame: &mut [u8]) -> Result<usize, CbpError> {
        let mut payload = vec![0u8; snapshots.len() * 48 + 32];
        let mut offset = 0;

        offset += encode_prefix_varint(snapshots.len() as u64, &mut payload[offset..offset + 9]);

        for s in snapshots {
            offset += encode_prefix_varint(s.player_id as u64, &mut payload[offset..offset + 9]);
            payload[offset..offset + 4].copy_from_slice(&s.x.to_bits().to_le_bytes());
            offset += 4;
            payload[offset..offset + 4].copy_from_slice(&s.y.to_bits().to_le_bytes());
            offset += 4;
            payload[offset..offset + 4].copy_from_slice(&s.z.to_bits().to_le_bytes());
            offset += 4;
            offset += encode_prefix_varint(s.health as u64, &mut payload[offset..offset + 9]);
            offset += encode_prefix_varint(s.flags as u64, &mut payload[offset..offset + 9]);
            offset += encode_prefix_varint(s.tick as u64, &mut payload[offset..offset + 9]);
        }

        let header = FrameHeader {
            profile: OptimizationProfile::Realtime,
            flags: FrameFlags {
                is_compressed: false,
                is_chunked: true,
                has_schema_hash: false,
            },
            payload_len: offset,
            schema_hash: None,
        };

        pack_frame(&header, &payload[..offset], out_frame)
    }

    /// Decodes a batch of `PlayerSnapshot`s from `frame_buf`.
    pub fn decode(frame_buf: &[u8], dst: &mut [PlayerSnapshot]) -> Result<usize, CbpError> {
        let (_header, payload) = unpack_frame(frame_buf)?;

        let mut offset = 0;
        let (count, v0) = decode_prefix_varint(&payload[offset..])?;
        offset += v0;
        let count = count as usize;

        if dst.len() < count {
            return Err(CbpError::PayloadDecodeError);
        }

        for item in dst.iter_mut().take(count) {
            let (player_id, v1) = decode_prefix_varint(&payload[offset..])?;
            offset += v1;
            let x = f32::from_bits(u32::from_le_bytes(
                payload[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let y = f32::from_bits(u32::from_le_bytes(
                payload[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let z = f32::from_bits(u32::from_le_bytes(
                payload[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
            let (health, v2) = decode_prefix_varint(&payload[offset..])?;
            offset += v2;
            let (flags, v3) = decode_prefix_varint(&payload[offset..])?;
            offset += v3;
            let (tick, v4) = decode_prefix_varint(&payload[offset..])?;
            offset += v4;

            *item = PlayerSnapshot {
                player_id: player_id as u32,
                x,
                y,
                z,
                health: health as u16,
                flags: flags as u16,
                tick: tick as u32,
            };
        }

        Ok(count)
    }
}
