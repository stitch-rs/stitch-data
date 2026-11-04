use core::hint::black_box;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use prost::Message;

use stitch_data::prelude::*;
use stitch_data_benches::scenarios::*;

// ============================================================================
// Scenario A: IP Intervals (1024 Intervals, 12,288 Bytes Raw AoS)
// ============================================================================

fn bench_cross_serializer_scenario_a_1k(c: &mut Criterion) {
    let mut group = c.benchmark_group("CrossSerializer/ScenarioA_1024Intervals");

    let count = 1024;
    let intervals = generate_ipatlas_intervals(count);
    let raw_size = count * 12; // 12,288 bytes
    group.throughput(Throughput::Bytes(raw_size as u64));

    // Pre-encode buffers for deserialize benchmarks
    let bincode_bytes = bincode::serialize(&intervals).unwrap();
    let postcard_bytes = postcard::to_allocvec(&intervals).unwrap();
    let rmp_bytes = rmp_serde::to_vec(&intervals).unwrap();

    let proto_intervals = intervals_to_proto(&intervals);
    let mut proto_bytes = Vec::new();
    proto_intervals.encode(&mut proto_bytes).unwrap();

    let mut cbp_scratch = vec![0u32; count * 2];
    let mut cbp_payload = vec![0u8; count * 16];
    let mut cbp_frame = vec![0u8; count * 16];
    let cbp_len = CbpReferenceIntervalContainer::encode(
        &intervals,
        &mut cbp_scratch,
        &mut cbp_payload,
        &mut cbp_frame,
    )
    .unwrap();

    // --- ENCODE BENCHMARKS ---

    // 1. Raw Memory AoS
    group.bench_function("encode/Raw_Memory_AoS", |b| {
        let mut dst = vec![
            Ipv4Interval {
                ip_from: 0,
                ip_to: 0,
                profile_id: 0
            };
            count
        ];
        b.iter(|| {
            dst.copy_from_slice(black_box(&intervals));
            black_box(dst.len())
        })
    });

    // 2. Bincode (Zero-alloc into pre-allocated buffer)
    group.bench_function("encode/Bincode", |b| {
        let mut buf = Vec::with_capacity(bincode_bytes.len());
        b.iter(|| {
            buf.clear();
            bincode::serialize_into(&mut buf, black_box(&intervals)).unwrap();
            black_box(buf.len())
        })
    });

    // 3. Postcard (Zero-alloc into pre-allocated slice)
    group.bench_function("encode/Postcard", |b| {
        let mut buf = vec![0u8; postcard_bytes.len() + 64];
        b.iter(|| {
            let slice = postcard::to_slice(black_box(&intervals), black_box(&mut buf)).unwrap();
            black_box(slice.len())
        })
    });

    // 4. Protobuf (prost)
    group.bench_function("encode/Protobuf_prost", |b| {
        let mut buf = Vec::with_capacity(proto_bytes.len());
        b.iter(|| {
            buf.clear();
            black_box(&proto_intervals).encode(&mut buf).unwrap();
            black_box(buf.len())
        })
    });

    // 5. MessagePack (rmp-serde)
    group.bench_function("encode/MessagePack", |b| {
        b.iter(|| black_box(rmp_serde::to_vec(black_box(&intervals)).unwrap()))
    });

    // 6. CBP Reference Columnar Container (All 3 columns + Delta + BitPack + Frame CRC32)
    group.bench_function("encode/CBP_Reference_Container", |b| {
        let mut scratch = vec![0u32; count * 2];
        let mut payload = vec![0u8; count * 16];
        let mut out = vec![0u8; count * 16];
        b.iter(|| {
            CbpReferenceIntervalContainer::encode(
                black_box(&intervals),
                black_box(&mut scratch),
                black_box(&mut payload),
                black_box(&mut out),
            )
            .unwrap()
        })
    });

    // --- DECODE BENCHMARKS ---

    // 1. Bincode Decode
    group.bench_function("decode/Bincode", |b| {
        b.iter(|| {
            let res: Vec<Ipv4Interval> = bincode::deserialize(black_box(&bincode_bytes)).unwrap();
            black_box(res)
        })
    });

    // 2. Postcard Decode
    group.bench_function("decode/Postcard", |b| {
        b.iter(|| {
            let res: Vec<Ipv4Interval> = postcard::from_bytes(black_box(&postcard_bytes)).unwrap();
            black_box(res)
        })
    });

    // 3. Protobuf (prost) Decode
    group.bench_function("decode/Protobuf_prost", |b| {
        b.iter(|| {
            let res = ProtoIpv4IntervalList::decode(black_box(&proto_bytes[..])).unwrap();
            black_box(res)
        })
    });

    // 4. MessagePack Decode
    group.bench_function("decode/MessagePack", |b| {
        b.iter(|| {
            let res: Vec<Ipv4Interval> = rmp_serde::from_slice(black_box(&rmp_bytes)).unwrap();
            black_box(res)
        })
    });

    // 5. CBP Reference Columnar Container Decode (Unpack Frame + CRC32 + Decompress 3 Columns)
    group.bench_function("decode/CBP_Reference_Container", |b| {
        let mut c1 = vec![0u32; count];
        let mut c2 = vec![0u32; count];
        let mut c3 = vec![0u32; count];
        let mut dst = vec![
            Ipv4Interval {
                ip_from: 0,
                ip_to: 0,
                profile_id: 0
            };
            count
        ];
        b.iter(|| {
            CbpReferenceIntervalContainer::decode(
                black_box(&cbp_frame[..cbp_len]),
                black_box(&mut c1),
                black_box(&mut c2),
                black_box(&mut c3),
                black_box(&mut dst),
            )
            .unwrap()
        })
    });

    group.finish();
}

// ============================================================================
// Scenario B: Game Telemetry Snapshot (Single Record, 24 Bytes Raw)
// ============================================================================

fn bench_cross_serializer_scenario_b_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("CrossSerializer/ScenarioB_SingleSnapshot");
    group.throughput(Throughput::Bytes(24));

    let snapshot = generate_player_snapshot(1337);

    let proto_snapshot = ProtoPlayerSnapshot {
        player_id: snapshot.player_id,
        x: snapshot.x,
        y: snapshot.y,
        z: snapshot.z,
        health: snapshot.health as u32,
        flags: snapshot.flags as u32,
        tick: snapshot.tick,
    };

    let bincode_buf = bincode::serialize(&snapshot).unwrap();
    let postcard_buf = postcard::to_allocvec(&snapshot).unwrap();
    let rmp_buf = rmp_serde::to_vec(&snapshot).unwrap();

    let mut proto_buf = Vec::new();
    proto_snapshot.encode(&mut proto_buf).unwrap();

    let mut cbp_buf = [0u8; 64];
    let cbp_len = CbpReferenceSnapshot::encode(&snapshot, &mut cbp_buf).unwrap();

    let mut derived_buf = [0u8; 64];
    let derived_len = snapshot.encode_record(&mut derived_buf).unwrap();

    let mut framed_derived_buf = [0u8; 64];
    let mut framed_scratch = [0u8; 64];
    let framed_derived_len = snapshot
        .pack_to_frame(
            OptimizationProfile::Balanced,
            &mut framed_scratch,
            &mut framed_derived_buf,
        )
        .unwrap();

    // --- ENCODE ---

    group.bench_function("encode/Raw_Memory_AoS", |b| {
        let mut dst = PlayerSnapshot {
            player_id: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            health: 0,
            flags: 0,
            tick: 0,
        };
        b.iter(|| {
            dst = *black_box(&snapshot);
            black_box(dst.player_id)
        })
    });

    group.bench_function("encode/Bincode", |b| {
        let mut buf = Vec::with_capacity(32);
        b.iter(|| {
            buf.clear();
            bincode::serialize_into(&mut buf, black_box(&snapshot)).unwrap();
            black_box(buf.len())
        })
    });

    group.bench_function("encode/Postcard", |b| {
        let mut buf = [0u8; 64];
        b.iter(|| {
            let slice = postcard::to_slice(black_box(&snapshot), black_box(&mut buf)).unwrap();
            black_box(slice.len())
        })
    });

    group.bench_function("encode/Protobuf_prost", |b| {
        let mut buf = Vec::with_capacity(32);
        b.iter(|| {
            buf.clear();
            black_box(&proto_snapshot).encode(&mut buf).unwrap();
            black_box(buf.len())
        })
    });

    group.bench_function("encode/MessagePack", |b| {
        b.iter(|| black_box(rmp_serde::to_vec(black_box(&snapshot)).unwrap()))
    });

    group.bench_function("encode/CBP_Reference_Snapshot", |b| {
        let mut out = [0u8; 64];
        b.iter(|| CbpReferenceSnapshot::encode(black_box(&snapshot), black_box(&mut out)).unwrap())
    });

    group.bench_function("encode/CBP_Derived_Record", |b| {
        let mut out = [0u8; 64];
        b.iter(|| {
            black_box(&snapshot)
                .encode_record(black_box(&mut out))
                .unwrap()
        })
    });

    group.bench_function("encode/CBP_Derived_Pack_With_Schema", |b| {
        let mut scratch = [0u8; 64];
        let mut out = [0u8; 64];
        b.iter(|| {
            black_box(&snapshot)
                .pack_to_frame(
                    OptimizationProfile::Balanced,
                    black_box(&mut scratch),
                    black_box(&mut out),
                )
                .unwrap()
        })
    });

    // --- DECODE ---

    group.bench_function("decode/Bincode", |b| {
        b.iter(|| {
            let res: PlayerSnapshot = bincode::deserialize(black_box(&bincode_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/Postcard", |b| {
        b.iter(|| {
            let res: PlayerSnapshot = postcard::from_bytes(black_box(&postcard_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/Protobuf_prost", |b| {
        b.iter(|| {
            let res = ProtoPlayerSnapshot::decode(black_box(&proto_buf[..])).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/MessagePack", |b| {
        b.iter(|| {
            let res: PlayerSnapshot = rmp_serde::from_slice(black_box(&rmp_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/CBP_Reference_Snapshot", |b| {
        b.iter(|| CbpReferenceSnapshot::decode(black_box(&cbp_buf[..cbp_len])).unwrap())
    });

    group.bench_function("decode/CBP_Derived_Record", |b| {
        b.iter(|| {
            let res: (PlayerSnapshot, usize) =
                PlayerSnapshot::decode_record(black_box(&derived_buf[..derived_len])).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/CBP_Derived_Unpack_With_Schema", |b| {
        b.iter(|| {
            let res: PlayerSnapshot = PlayerSnapshot::unpack_from_frame(black_box(
                &framed_derived_buf[..framed_derived_len],
            ))
            .unwrap();
            black_box(res)
        })
    });

    group.finish();
}

// ============================================================================
// Scenario B2: Batch of 64 Snapshots (1,536 Bytes Raw)
// ============================================================================

fn bench_cross_serializer_scenario_b_batch64(c: &mut Criterion) {
    let mut group = c.benchmark_group("CrossSerializer/ScenarioB_Batch64Snapshots");
    let count = 64;
    let raw_size = count * 24;
    group.throughput(Throughput::Bytes(raw_size as u64));

    let batch = generate_player_snapshots(count);

    let bincode_buf = bincode::serialize(&batch).unwrap();
    let postcard_buf = postcard::to_allocvec(&batch).unwrap();
    let rmp_buf = rmp_serde::to_vec(&batch).unwrap();

    let proto_batch = snapshots_to_proto(&batch);
    let mut proto_buf = Vec::new();
    proto_batch.encode(&mut proto_buf).unwrap();

    let mut cbp_buf = vec![0u8; count * 48];
    let cbp_len = CbpReferenceSnapshotBatch::encode(&batch, &mut cbp_buf).unwrap();

    let mut builder_init =
        ColumnarBlockBuilder::<PlayerSnapshot, 64>::new(OptimizationProfile::Balanced);
    for s in &batch {
        builder_init.push(*s).unwrap();
    }
    let mut builder_scratch_init = vec![0u8; count * 48];
    let mut builder_frame_buf = vec![0u8; count * 48];
    let builder_frame_len = builder_init
        .flush(&mut builder_scratch_init, &mut builder_frame_buf)
        .unwrap();

    // ENCODE
    group.bench_function("encode/Bincode", |b| {
        b.iter(|| black_box(bincode::serialize(black_box(&batch)).unwrap()))
    });

    group.bench_function("encode/Postcard", |b| {
        b.iter(|| black_box(postcard::to_allocvec(black_box(&batch)).unwrap()))
    });

    group.bench_function("encode/Protobuf_prost", |b| {
        let mut buf = Vec::with_capacity(proto_buf.len());
        b.iter(|| {
            buf.clear();
            black_box(&proto_batch).encode(&mut buf).unwrap();
            black_box(buf.len())
        })
    });

    group.bench_function("encode/MessagePack", |b| {
        b.iter(|| black_box(rmp_serde::to_vec(black_box(&batch)).unwrap()))
    });

    group.bench_function("encode/CBP_Reference_Batch", |b| {
        let mut out = vec![0u8; count * 48];
        b.iter(|| {
            CbpReferenceSnapshotBatch::encode(black_box(&batch), black_box(&mut out)).unwrap()
        })
    });

    group.bench_function("encode/CBP_ColumnarBlockBuilder_64", |b| {
        let mut scratch = vec![0u8; count * 48];
        let mut out = vec![0u8; count * 48];
        b.iter(|| {
            let mut builder =
                ColumnarBlockBuilder::<PlayerSnapshot, 64>::new(OptimizationProfile::Balanced);
            for s in black_box(&batch) {
                let _ = builder.push(*s);
            }
            builder
                .flush(black_box(&mut scratch), black_box(&mut out))
                .unwrap()
        })
    });

    // DECODE
    group.bench_function("decode/Bincode", |b| {
        b.iter(|| {
            let res: Vec<PlayerSnapshot> = bincode::deserialize(black_box(&bincode_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/Postcard", |b| {
        b.iter(|| {
            let res: Vec<PlayerSnapshot> = postcard::from_bytes(black_box(&postcard_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/Protobuf_prost", |b| {
        b.iter(|| {
            let res = ProtoPlayerSnapshotBatch::decode(black_box(&proto_buf[..])).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/MessagePack", |b| {
        b.iter(|| {
            let res: Vec<PlayerSnapshot> = rmp_serde::from_slice(black_box(&rmp_buf)).unwrap();
            black_box(res)
        })
    });

    group.bench_function("decode/CBP_Reference_Batch", |b| {
        let mut dst = vec![
            PlayerSnapshot {
                player_id: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                health: 0,
                flags: 0,
                tick: 0,
            };
            count
        ];
        b.iter(|| {
            CbpReferenceSnapshotBatch::decode(black_box(&cbp_buf[..cbp_len]), black_box(&mut dst))
                .unwrap()
        })
    });

    group.bench_function("decode/CBP_ColumnarBlockBuilder_64", |b| {
        let mut dst = [PlayerSnapshot {
            player_id: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            health: 0,
            flags: 0,
            tick: 0,
        }; 64];
        b.iter(|| {
            let (header, payload) =
                unpack_frame(black_box(&builder_frame_buf[..builder_frame_len])).unwrap();
            assert_eq!(header.schema_hash, Some(PlayerSnapshot::SCHEMA_HASH));
            PlayerSnapshot::decode_batch(payload, black_box(&mut dst)).unwrap()
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cross_serializer_scenario_a_1k,
    bench_cross_serializer_scenario_b_single,
    bench_cross_serializer_scenario_b_batch64
);
criterion_main!(benches);
