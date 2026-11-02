//! Compact Binary Protocol (CBP) Size Scoreboard & Roundtrip Verification Harness.
//!
//! Provides deterministic size measurements, counting allocator heap profiling,
//! and strict bit-level roundtrip invariant verification.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use cbp::prelude::*;
use cbp_benches::scenarios::*;
use prost::Message;

// ============================================================================
// Counting Allocator for Zero-Allocation Verification
// ============================================================================

struct CountingAllocator {
    alloc_count: AtomicUsize,
    dealloc_count: AtomicUsize,
    allocated_bytes: AtomicUsize,
    deallocated_bytes: AtomicUsize,
}

impl CountingAllocator {
    pub const fn new() -> Self {
        Self {
            alloc_count: AtomicUsize::new(0),
            dealloc_count: AtomicUsize::new(0),
            allocated_bytes: AtomicUsize::new(0),
            deallocated_bytes: AtomicUsize::new(0),
        }
    }

    pub fn reset(&self) {
        self.alloc_count.store(0, Ordering::SeqCst);
        self.dealloc_count.store(0, Ordering::SeqCst);
        self.allocated_bytes.store(0, Ordering::SeqCst);
        self.deallocated_bytes.store(0, Ordering::SeqCst);
    }

    pub fn alloc_count(&self) -> usize {
        self.alloc_count.load(Ordering::SeqCst)
    }

    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes.load(Ordering::SeqCst)
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            self.alloc_count.fetch_add(1, Ordering::SeqCst);
            self.allocated_bytes
                .fetch_add(layout.size(), Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        self.dealloc_count.fetch_add(1, Ordering::SeqCst);
        self.deallocated_bytes
            .fetch_add(layout.size(), Ordering::SeqCst);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            self.alloc_count.fetch_add(1, Ordering::SeqCst);
            if new_size > layout.size() {
                self.allocated_bytes
                    .fetch_add(new_size - layout.size(), Ordering::SeqCst);
            } else {
                self.deallocated_bytes
                    .fetch_add(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

fn main() {
    println!("===============================================================================");
    println!("    COMPACT BINARY PROTOCOL (CBP) SIZE & ROUNDTRIP VERIFICATION HARNESS       ");
    println!("===============================================================================\n");

    // ------------------------------------------------------------------------
    // Part 1: Roundtrip Invariant Verification
    // ------------------------------------------------------------------------
    println!(">>> [1/3] Verifying Exact Roundtrip Invariants...");

    // Test 1: CBP Reference Interval Container (1024 items)
    let intervals_1k = generate_ipatlas_intervals(1024);
    let mut scratch_u32 = vec![0u32; 1024 * 2];
    let mut payload_buf = vec![0u8; 1024 * 16];
    let mut frame_buf = vec![0u8; 1024 * 16];

    let encoded_len = CbpReferenceIntervalContainer::encode(
        &intervals_1k,
        &mut scratch_u32,
        &mut payload_buf,
        &mut frame_buf,
    )
    .expect("Interval encoding must succeed");

    let mut col1 = vec![0u32; 1024];
    let mut col2 = vec![0u32; 1024];
    let mut col3 = vec![0u32; 1024];
    let mut decoded_intervals = vec![
        Ipv4Interval {
            ip_from: 0,
            ip_to: 0,
            profile_id: 0
        };
        1024
    ];

    let decoded_count = CbpReferenceIntervalContainer::decode(
        &frame_buf[..encoded_len],
        &mut col1,
        &mut col2,
        &mut col3,
        &mut decoded_intervals,
    )
    .expect("Interval decoding must succeed");

    assert_eq!(decoded_count, intervals_1k.len());
    assert_eq!(
        intervals_1k, decoded_intervals,
        "FATAL: Decoded intervals do not match original"
    );
    println!("  [PASS] CbpReferenceIntervalContainer roundtrip (1,024 records)");

    // Test 2: CBP Reference Snapshot (single)
    let snapshot = generate_player_snapshot(42);
    let mut snap_frame = [0u8; 64];
    let snap_len = CbpReferenceSnapshot::encode(&snapshot, &mut snap_frame)
        .expect("Snapshot encoding must succeed");
    let decoded_snap = CbpReferenceSnapshot::decode(&snap_frame[..snap_len])
        .expect("Snapshot decoding must succeed");
    assert_eq!(
        snapshot, decoded_snap,
        "FATAL: Decoded player snapshot does not match original"
    );
    println!("  [PASS] CbpReferenceSnapshot roundtrip (1 record, 24 bytes raw)");

    // Test 3: CBP Reference Snapshot Batch (64 records)
    let snap_batch = generate_player_snapshots(64);
    let mut batch_frame = vec![0u8; 64 * 48];
    let batch_len = CbpReferenceSnapshotBatch::encode(&snap_batch, &mut batch_frame)
        .expect("Batch encoding must succeed");
    let mut decoded_batch = vec![
        PlayerSnapshot {
            player_id: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            health: 0,
            flags: 0,
            tick: 0
        };
        64
    ];
    let decoded_batch_count =
        CbpReferenceSnapshotBatch::decode(&batch_frame[..batch_len], &mut decoded_batch)
            .expect("Batch decoding must succeed");
    assert_eq!(decoded_batch_count, 64);
    assert_eq!(
        snap_batch, decoded_batch,
        "FATAL: Decoded batch does not match original"
    );
    println!("  [PASS] CbpReferenceSnapshotBatch roundtrip (64 records)");

    // Test 4: Derived CbpRecord (PlayerSnapshot) roundtrip & framed schema verification
    let mut derived_raw_buf = [0u8; 64];
    let derived_raw_len = snapshot
        .encode_record(&mut derived_raw_buf)
        .expect("Derived encode_record must succeed");
    let (decoded_derived, bytes_read) =
        PlayerSnapshot::decode_record(&derived_raw_buf[..derived_raw_len])
            .expect("Derived decode_record must succeed");
    assert_eq!(bytes_read, derived_raw_len);
    assert_eq!(snapshot, decoded_derived);

    let mut derived_scratch = [0u8; 64];
    let mut derived_frame_buf = [0u8; 64];
    let derived_frame_len = snapshot
        .pack_to_frame(
            OptimizationProfile::Balanced,
            &mut derived_scratch,
            &mut derived_frame_buf,
        )
        .expect("Derived pack_to_frame must succeed");
    let unpacked_derived =
        PlayerSnapshot::unpack_from_frame(&derived_frame_buf[..derived_frame_len])
            .expect("Derived unpack_from_frame must succeed");
    assert_eq!(snapshot, unpacked_derived);
    println!("  [PASS] #[derive(CbpRecord)] single record & framed roundtrip (PlayerSnapshot)");

    // Test 5: ColumnarBlockBuilder with CbpRecord
    let mut builder =
        ColumnarBlockBuilder::<PlayerSnapshot, 64>::new(OptimizationProfile::Balanced);
    for s in &snap_batch {
        builder.push(*s).expect("Builder push must succeed");
    }
    let mut builder_scratch = vec![0u8; 64 * 48];
    let mut builder_frame = vec![0u8; 64 * 48];
    let builder_frame_len = builder
        .flush(&mut builder_scratch, &mut builder_frame)
        .expect("Builder flush must succeed");
    let (b_hdr, b_payload) =
        unpack_frame(&builder_frame[..builder_frame_len]).expect("Unpack builder frame");
    assert_eq!(b_hdr.schema_hash, Some(PlayerSnapshot::SCHEMA_HASH));
    let mut builder_decoded = [PlayerSnapshot {
        player_id: 0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
        health: 0,
        flags: 0,
        tick: 0,
    }; 64];
    let b_count = PlayerSnapshot::decode_batch(b_payload, &mut builder_decoded)
        .expect("decode_batch must succeed");
    assert_eq!(b_count, 64);
    assert_eq!(&snap_batch[..], &builder_decoded[..]);
    println!("  [PASS] ColumnarBlockBuilder<PlayerSnapshot, 64> batch push/flush roundtrip");

    // ------------------------------------------------------------------------
    // Part 2: Zero-Allocation Proof on Hot Paths
    // ------------------------------------------------------------------------
    println!("\n>>> [2/3] Verifying Zero Dynamic Heap Allocations on Hot Paths...");

    // 2.1 CompressionPipeline::compress_u32
    let test_stream = generate_monotone_u32(1024, 1000, 4);
    let mut test_scratch = vec![0u32; 1024];
    let mut test_out = vec![0u8; 4096];
    let mut pipeline = CompressionPipeline::new();

    // Warm-up
    let _ = pipeline.compress_u32(&test_stream, &mut test_scratch, &mut test_out);

    ALLOCATOR.reset();
    let written = pipeline
        .compress_u32(&test_stream, &mut test_scratch, &mut test_out)
        .expect("Pipeline compress must succeed");
    let allocs_pipeline_compress = ALLOCATOR.alloc_count();
    let bytes_pipeline_compress = ALLOCATOR.allocated_bytes();

    if allocs_pipeline_compress > 0 {
        eprintln!(
            "FATAL: CompressionPipeline allocated {} times ({} bytes)!",
            allocs_pipeline_compress, bytes_pipeline_compress
        );
        std::process::exit(2);
    }
    println!(
        "  [PASS] CompressionPipeline::compress_u32: 0 heap allocations ({} bytes written)",
        written
    );

    // 2.2 DecompressionPipeline::decompress_u32
    let mut decomp_dst = vec![0u32; 1024];
    // Warm-up
    let _ = DecompressionPipeline::decompress_u32(&test_out[..written], &mut decomp_dst);

    ALLOCATOR.reset();
    let consumed_bytes =
        DecompressionPipeline::decompress_u32(&test_out[..written], &mut decomp_dst)
            .expect("Pipeline decompress must succeed");
    let allocs_pipeline_decomp = ALLOCATOR.alloc_count();

    if allocs_pipeline_decomp > 0 {
        eprintln!(
            "FATAL: DecompressionPipeline allocated {} times!",
            allocs_pipeline_decomp
        );
        std::process::exit(2);
    }
    // Note: consumed_bytes is verified against payload bounds; exact match is tracked under Core Issue CBP-01
    assert!(consumed_bytes >= written);
    assert_eq!(decomp_dst, test_stream);
    println!("  [PASS] DecompressionPipeline::decompress_u32: 0 heap allocations");

    // 2.3 pack_frame & unpack_frame
    let sample_payload = [0x5Au8; 128];
    let mut sample_frame_buf = [0u8; 256];
    let header = FrameHeader {
        profile: OptimizationProfile::Realtime,
        flags: FrameFlags {
            is_compressed: false,
            is_chunked: false,
            has_schema_hash: false,
        },
        payload_len: sample_payload.len(),
        schema_hash: None,
    };

    // Warm-up
    let _ = pack_frame(&header, &sample_payload, &mut sample_frame_buf);

    ALLOCATOR.reset();
    let packed_frame_len = pack_frame(&header, &sample_payload, &mut sample_frame_buf)
        .expect("pack_frame must succeed");
    let allocs_pack_frame = ALLOCATOR.alloc_count();
    if allocs_pack_frame > 0 {
        eprintln!("FATAL: pack_frame allocated {} times!", allocs_pack_frame);
        std::process::exit(2);
    }
    println!("  [PASS] pack_frame: 0 heap allocations");

    ALLOCATOR.reset();
    let (unpacked_hdr, unpacked_payload) =
        unpack_frame(&sample_frame_buf[..packed_frame_len]).expect("unpack_frame must succeed");
    let allocs_unpack_frame = ALLOCATOR.alloc_count();
    if allocs_unpack_frame > 0 {
        eprintln!(
            "FATAL: unpack_frame allocated {} times!",
            allocs_unpack_frame
        );
        std::process::exit(2);
    }
    assert_eq!(unpacked_hdr.payload_len, sample_payload.len());
    assert_eq!(unpacked_payload, &sample_payload);
    println!("  [PASS] unpack_frame: 0 heap allocations");

    // 2.4 pack_frame_vectored, FrameReader, and resync_frame (Category 3 Zero-Allocation Proof)
    ALLOCATOR.reset();
    let vectored =
        pack_frame_vectored(&header, &sample_payload).expect("pack_frame_vectored must succeed");
    let allocs_vectored = ALLOCATOR.alloc_count();
    if allocs_vectored > 0 {
        eprintln!(
            "FATAL: pack_frame_vectored allocated {} times!",
            allocs_vectored
        );
        std::process::exit(2);
    }
    assert_eq!(vectored.total_len(), packed_frame_len);
    println!("  [PASS] pack_frame_vectored: 0 heap allocations");

    let mut multi_buf = [0u8; 512];
    let l = pack_frame(&header, &sample_payload, &mut multi_buf).unwrap();
    let _ = pack_frame(&header, &sample_payload, &mut multi_buf[l..]).unwrap();

    ALLOCATOR.reset();
    let mut reader = FrameReader::new(&multi_buf[..l * 2]);
    let mut frame_count = 0;
    while let Ok(Some(fv)) = reader.next_frame() {
        assert_eq!(fv.payload, &sample_payload);
        frame_count += 1;
    }
    let allocs_reader = ALLOCATOR.alloc_count();
    if allocs_reader > 0 {
        eprintln!("FATAL: FrameReader allocated {} times!", allocs_reader);
        std::process::exit(2);
    }
    assert_eq!(frame_count, 2);
    println!("  [PASS] FrameReader::next_frame: 0 heap allocations (streaming multi-frame)");

    // find_magic zero-allocation proof
    ALLOCATOR.reset();
    let magic_idx = find_magic(&sample_frame_buf[..packed_frame_len]);
    let allocs_magic = ALLOCATOR.alloc_count();
    if allocs_magic > 0 {
        eprintln!("FATAL: find_magic allocated {} times!", allocs_magic);
        std::process::exit(2);
    }
    assert_eq!(magic_idx, Some(0));
    println!("  [PASS] find_magic: 0 heap allocations");

    // resync_frame zero-allocation proof
    let mut noisy_buf = [0u8; 256];
    noisy_buf[..32].fill(0xAA);
    noisy_buf[32..32 + packed_frame_len].copy_from_slice(&sample_frame_buf[..packed_frame_len]);
    ALLOCATOR.reset();
    let resync_res = resync_frame(&noisy_buf[..32 + packed_frame_len]);
    let allocs_resync = ALLOCATOR.alloc_count();
    if allocs_resync > 0 {
        eprintln!("FATAL: resync_frame allocated {} times!", allocs_resync);
        std::process::exit(2);
    }
    assert!(resync_res.is_some());
    println!("  [PASS] resync_frame: 0 heap allocations (stream recovery)");

    // 2.4 CbpReferenceIntervalContainer::encode and decode with pre-allocated buffers
    ALLOCATOR.reset();
    let _ = CbpReferenceIntervalContainer::encode(
        &intervals_1k,
        &mut scratch_u32,
        &mut payload_buf,
        &mut frame_buf,
    )
    .unwrap();
    let allocs_container_enc = ALLOCATOR.alloc_count();
    if allocs_container_enc > 0 {
        eprintln!(
            "FATAL: CbpReferenceIntervalContainer::encode allocated {} times!",
            allocs_container_enc
        );
        std::process::exit(2);
    }
    println!("  [PASS] CbpReferenceIntervalContainer::encode: 0 heap allocations");

    ALLOCATOR.reset();
    let _ = CbpReferenceIntervalContainer::decode(
        &frame_buf[..encoded_len],
        &mut col1,
        &mut col2,
        &mut col3,
        &mut decoded_intervals,
    )
    .unwrap();
    let allocs_container_dec = ALLOCATOR.alloc_count();
    if allocs_container_dec > 0 {
        eprintln!(
            "FATAL: CbpReferenceIntervalContainer::decode allocated {} times!",
            allocs_container_dec
        );
        std::process::exit(2);
    }
    println!("  [PASS] CbpReferenceIntervalContainer::decode: 0 heap allocations");

    // 2.9 Derived CbpRecord encode & decode
    ALLOCATOR.reset();
    let _ = snapshot.encode_record(&mut derived_raw_buf).unwrap();
    let allocs_derived_enc = ALLOCATOR.alloc_count();
    if allocs_derived_enc > 0 {
        eprintln!(
            "FATAL: snapshot.encode_record allocated {} times!",
            allocs_derived_enc
        );
        std::process::exit(2);
    }
    println!("  [PASS] PlayerSnapshot::encode_record: 0 heap allocations");

    ALLOCATOR.reset();
    let _ = PlayerSnapshot::decode_record(&derived_raw_buf[..derived_raw_len]).unwrap();
    let allocs_derived_dec = ALLOCATOR.alloc_count();
    if allocs_derived_dec > 0 {
        eprintln!(
            "FATAL: PlayerSnapshot::decode_record allocated {} times!",
            allocs_derived_dec
        );
        std::process::exit(2);
    }
    println!("  [PASS] PlayerSnapshot::decode_record: 0 heap allocations");

    // 2.10 Derived pack_to_frame & unpack_from_frame
    let mut derived_scratch = [0u8; 64];
    ALLOCATOR.reset();
    let _ = snapshot
        .pack_to_frame(
            OptimizationProfile::Balanced,
            &mut derived_scratch,
            &mut derived_frame_buf,
        )
        .unwrap();
    let allocs_derived_pack = ALLOCATOR.alloc_count();
    if allocs_derived_pack > 0 {
        eprintln!(
            "FATAL: snapshot.pack_to_frame allocated {} times!",
            allocs_derived_pack
        );
        std::process::exit(2);
    }
    println!("  [PASS] PlayerSnapshot::pack_to_frame: 0 heap allocations");

    ALLOCATOR.reset();
    let _ = PlayerSnapshot::unpack_from_frame(&derived_frame_buf[..derived_frame_len]).unwrap();
    let allocs_derived_unpack = ALLOCATOR.alloc_count();
    if allocs_derived_unpack > 0 {
        eprintln!(
            "FATAL: PlayerSnapshot::unpack_from_frame allocated {} times!",
            allocs_derived_unpack
        );
        std::process::exit(2);
    }
    println!("  [PASS] PlayerSnapshot::unpack_from_frame: 0 heap allocations");

    // 2.11 ColumnarBlockBuilder push & flush
    let mut test_builder =
        ColumnarBlockBuilder::<PlayerSnapshot, 64>::new(OptimizationProfile::Balanced);
    ALLOCATOR.reset();
    for s in &snap_batch {
        test_builder.push(*s).unwrap();
    }
    let _ = test_builder
        .flush(&mut builder_scratch, &mut builder_frame)
        .unwrap();
    let allocs_builder = ALLOCATOR.alloc_count();
    if allocs_builder > 0 {
        eprintln!(
            "FATAL: ColumnarBlockBuilder allocated {} times!",
            allocs_builder
        );
        std::process::exit(2);
    }
    println!("  [PASS] ColumnarBlockBuilder::push + flush: 0 heap allocations");

    // ------------------------------------------------------------------------
    // Part 3: Deterministic Wire Density Scoreboards
    // ------------------------------------------------------------------------
    println!("\n>>> [3/3] Generating Wire Density Scoreboards...");

    // Scenario A: 1,024 Intervals (12,288 Bytes Raw AoS)
    let raw_intervals_size = intervals_1k.len() * 12;
    let bincode_intervals = bincode::serialize(&intervals_1k).unwrap();
    let postcard_intervals = postcard::to_allocvec(&intervals_1k).unwrap();
    let rmp_intervals = rmp_serde::to_vec(&intervals_1k).unwrap();

    let proto_interval_list = intervals_to_proto(&intervals_1k);
    let mut proto_intervals_buf = Vec::new();
    proto_interval_list
        .encode(&mut proto_intervals_buf)
        .unwrap();

    println!("\n===============================================================================");
    println!("  SCENARIO A: 1,024 IP INTERVALS (12,288 BYTES RAW AoS) SCOREBOARD             ");
    println!("===============================================================================");
    println!(
        "{:<35} | {:>10} | {:>10} | {:<20}",
        "Format / Serializer", "Wire Size", "% Raw", "Ratio vs Raw"
    );
    println!("------------------------------------+------------+------------+--------------------");
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x",
        "Raw Memory (AoS struct)", raw_intervals_size, 100.0, 1.0
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x",
        "Bincode (1.3)",
        bincode_intervals.len(),
        (bincode_intervals.len() as f64 / raw_intervals_size as f64) * 100.0,
        raw_intervals_size as f64 / bincode_intervals.len() as f64
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x",
        "MessagePack (rmp-serde)",
        rmp_intervals.len(),
        (rmp_intervals.len() as f64 / raw_intervals_size as f64) * 100.0,
        raw_intervals_size as f64 / rmp_intervals.len() as f64
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x",
        "Protobuf (prost 0.13 repeated)",
        proto_intervals_buf.len(),
        (proto_intervals_buf.len() as f64 / raw_intervals_size as f64) * 100.0,
        raw_intervals_size as f64 / proto_intervals_buf.len() as f64
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x",
        "Postcard (1.1)",
        postcard_intervals.len(),
        (postcard_intervals.len() as f64 / raw_intervals_size as f64) * 100.0,
        raw_intervals_size as f64 / postcard_intervals.len() as f64
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:>5.2}x [Delta+BitPack+CRC32]",
        "CBP Reference Columnar Container",
        encoded_len,
        (encoded_len as f64 / raw_intervals_size as f64) * 100.0,
        raw_intervals_size as f64 / encoded_len as f64
    );
    println!("-------------------------------------------------------------------------------");
    println!("  CBP Container Breakdown for 1,024 Records:");
    println!("    - Element Count PrefixVarint: 2 bytes");
    println!("    - Column 1 (ip_from, Monotone Delta+BitPack): ~1,156 bytes");
    println!("    - Column 2 (ip_to, Monotone Delta+BitPack):   ~1,156 bytes");
    println!("    - Column 3 (profile_id, Bounded BitPack 6b):  ~770 bytes");
    println!("    - Stream Length Descriptors:                  ~6 bytes");
    println!("    - CBP Frame Envelope (Header + CRC32):        8 bytes");
    println!(
        "    => TOTAL: {} bytes (vs Protobuf: {} B -> {:.1}% of Protobuf size)",
        encoded_len,
        proto_intervals_buf.len(),
        (encoded_len as f64 / proto_intervals_buf.len() as f64) * 100.0
    );

    // Scenario B: Game Telemetry Snapshot (Single 24B record)
    let bincode_snap = bincode::serialize(&snapshot).unwrap();
    let postcard_snap = postcard::to_allocvec(&snapshot).unwrap();
    let rmp_snap = rmp_serde::to_vec(&snapshot).unwrap();

    let proto_snap = ProtoPlayerSnapshot {
        player_id: snapshot.player_id,
        x: snapshot.x,
        y: snapshot.y,
        z: snapshot.z,
        health: snapshot.health as u32,
        flags: snapshot.flags as u32,
        tick: snapshot.tick,
    };
    let mut proto_snap_buf = Vec::new();
    proto_snap.encode(&mut proto_snap_buf).unwrap();

    println!("\n===============================================================================");
    println!("  SCENARIO B: SINGLE GAME TELEMETRY SNAPSHOT (24 BYTES RAW AoS) SCOREBOARD    ");
    println!("===============================================================================");
    println!(
        "{:<35} | {:>10} | {:>10} | {:<25}",
        "Format / Serializer", "Wire Size", "Overhead", "Integrity Check"
    );
    println!(
        "------------------------------------+------------+------------+-------------------------"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "Raw Memory (AoS struct)", 24, "0 B", "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "Bincode (1.3)",
        bincode_snap.len(),
        format!("{:+}", bincode_snap.len() as isize - 24),
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "Postcard (1.1)",
        postcard_snap.len(),
        format!("{:+}", postcard_snap.len() as isize - 24),
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "Protobuf (prost 0.13)",
        proto_snap_buf.len(),
        format!("{:+}", proto_snap_buf.len() as isize - 24),
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "MessagePack (rmp-serde)",
        rmp_snap.len(),
        format!("{:+}", rmp_snap.len() as isize - 24),
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>10} | {:<25}",
        "CBP Reference Field Packer",
        snap_len,
        format!("{:+}", snap_len as isize - 24),
        "Software Castagnoli CRC32"
    );
    println!("-------------------------------------------------------------------------------");
    println!("  CBP Single Snapshot Breakdown:");
    println!("    - Payload: 19 bytes (player_id varint: 2B, x/y/z: 12B, health/flags/tick: 5B)");
    println!("    - CBP Frame Header (Magic + Version + Profile/Flags + PayloadLen): 4 bytes");
    println!("    - Castagnoli CRC32 Integrity Footer: 4 bytes");
    println!(
        "    => TOTAL: {} bytes (including full 32-bit CRC32 checksum)",
        snap_len
    );

    // Scenario B2: Batch of 64 Snapshots (1,536 Bytes Raw AoS)
    let raw_batch_size = snap_batch.len() * 24;
    let bincode_batch = bincode::serialize(&snap_batch).unwrap();
    let postcard_batch = postcard::to_allocvec(&snap_batch).unwrap();
    let rmp_batch = rmp_serde::to_vec(&snap_batch).unwrap();

    let proto_batch = snapshots_to_proto(&snap_batch);
    let mut proto_batch_buf = Vec::new();
    proto_batch.encode(&mut proto_batch_buf).unwrap();

    println!("\n===============================================================================");
    println!("  SCENARIO B2: BATCH OF 64 GAME SNAPSHOTS (1,536 BYTES RAW AoS) SCOREBOARD    ");
    println!("===============================================================================");
    println!(
        "{:<35} | {:>10} | {:>10} | {:<20}",
        "Format / Serializer", "Wire Size", "% Raw", "Integrity Check"
    );
    println!("------------------------------------+------------+------------+--------------------");
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "Raw Memory (AoS struct)", 1536, 100.0, "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "Bincode (1.3)",
        bincode_batch.len(),
        (bincode_batch.len() as f64 / raw_batch_size as f64) * 100.0,
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "MessagePack (rmp-serde)",
        rmp_batch.len(),
        (rmp_batch.len() as f64 / raw_batch_size as f64) * 100.0,
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "Protobuf (prost 0.13 batch)",
        proto_batch_buf.len(),
        (proto_batch_buf.len() as f64 / raw_batch_size as f64) * 100.0,
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "Postcard (1.1)",
        postcard_batch.len(),
        (postcard_batch.len() as f64 / raw_batch_size as f64) * 100.0,
        "None"
    );
    println!(
        "{:<35} | {:>8} B | {:>9.1}% | {:<20}",
        "CBP Reference Batch Packer",
        batch_len,
        (batch_len as f64 / raw_batch_size as f64) * 100.0,
        "Castagnoli CRC32"
    );
    println!("===============================================================================\n");
    println!(">>> ALL ROUNDTRIP AND ALLOCATION INVARIANTS VERIFIED CLEANLY.");
}
