use core::hint::black_box;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use cbp::prelude::*;

fn bench_frame_pack_unpack(c: &mut Criterion) {
    let mut group = c.benchmark_group("Framing/Pack_Unpack");

    let payload_sizes: &[usize] = &[24, 1024, 65536, 1048576];

    for &size in payload_sizes {
        let label = match size {
            24 => "24B",
            1024 => "1KiB",
            65536 => "64KiB",
            _ => "1MiB",
        };

        group.throughput(Throughput::Bytes(size as u64));

        let payload = vec![0x3Cu8; size];
        let mut frame_buf = vec![0u8; size + 64];

        let header = FrameHeader {
            profile: OptimizationProfile::Realtime,
            flags: FrameFlags {
                is_compressed: false,
                is_chunked: false,
                has_schema_hash: false,
            },
            payload_len: size,
            schema_hash: None,
        };

        let frame_len = pack_frame(&header, &payload, &mut frame_buf).unwrap();

        group.bench_with_input(
            BenchmarkId::new("pack_frame_contiguous", label),
            &size,
            |b, _| {
                let mut out = vec![0u8; size + 64];
                b.iter(|| {
                    pack_frame(black_box(&header), black_box(&payload), black_box(&mut out))
                        .unwrap()
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("pack_frame_vectored", label),
            &size,
            |b, _| b.iter(|| pack_frame_vectored(black_box(&header), black_box(&payload)).unwrap()),
        );

        group.bench_with_input(BenchmarkId::new("unpack_frame", label), &size, |b, _| {
            b.iter(|| unpack_frame(black_box(&frame_buf[..frame_len])).unwrap())
        });
    }

    group.finish();
}

fn bench_frame_variations(c: &mut Criterion) {
    let mut group = c.benchmark_group("Framing/Header_Variations");

    let payload = vec![0x7Eu8; 64];
    let mut out_buf = vec![0u8; 128];

    // 1. Without Schema Hash
    let hdr_no_hash = FrameHeader {
        profile: OptimizationProfile::Realtime,
        flags: FrameFlags {
            is_compressed: false,
            is_chunked: false,
            has_schema_hash: false,
        },
        payload_len: 64,
        schema_hash: None,
    };

    group.bench_function("pack/no_schema_hash_64B", |b| {
        b.iter(|| {
            pack_frame(
                black_box(&hdr_no_hash),
                black_box(&payload),
                black_box(&mut out_buf),
            )
            .unwrap()
        })
    });

    // 2. With Schema Hash
    let hdr_with_hash = FrameHeader {
        profile: OptimizationProfile::Realtime,
        flags: FrameFlags {
            is_compressed: false,
            is_chunked: false,
            has_schema_hash: true,
        },
        payload_len: 64,
        schema_hash: Some(0xDEAD_BEEF_CAFE_BABE),
    };

    group.bench_function("pack/with_schema_hash_64B", |b| {
        b.iter(|| {
            pack_frame(
                black_box(&hdr_with_hash),
                black_box(&payload),
                black_box(&mut out_buf),
            )
            .unwrap()
        })
    });

    // 3. Profile variations
    let profiles = [
        (OptimizationProfile::Direct, "Direct"),
        (OptimizationProfile::Realtime, "Realtime"),
        (OptimizationProfile::Balanced, "Balanced"),
        (OptimizationProfile::MaxDensity, "MaxDensity"),
    ];

    for (p, p_label) in profiles {
        let hdr = FrameHeader {
            profile: p,
            flags: FrameFlags::default(),
            payload_len: 64,
            schema_hash: None,
        };
        group.bench_function(format!("pack/profile_{}_64B", p_label), |b| {
            b.iter(|| {
                pack_frame(
                    black_box(&hdr),
                    black_box(&payload),
                    black_box(&mut out_buf),
                )
                .unwrap()
            })
        });
    }

    group.finish();
}

fn bench_frame_crc_breakdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("Framing/CRC_Overhead_Breakdown");

    let size = 65536; // 64 KiB
    group.throughput(Throughput::Bytes(size as u64));

    let payload = vec![0x55u8; size];
    let mut frame_buf = vec![0u8; size + 64];

    let header = FrameHeader {
        profile: OptimizationProfile::Balanced,
        flags: FrameFlags::default(),
        payload_len: size,
        schema_hash: None,
    };

    // Total pack_frame (Header + Payload copy + Software CRC32)
    group.bench_function("total_pack_frame_64KiB", |b| {
        b.iter(|| {
            pack_frame(
                black_box(&header),
                black_box(&payload),
                black_box(&mut frame_buf),
            )
            .unwrap()
        })
    });

    // Pure CRC32 component
    group.bench_function("pure_crc32_component_64KiB", |b| {
        b.iter(|| crc32_castagnoli(black_box(&payload)))
    });

    // Frame Header + Payload copy only (without CRC calculation)
    group.bench_function("payload_copy_and_header_only_64KiB", |b| {
        let mut buf = vec![0u8; size + 64];
        b.iter(|| {
            let hlen = header.write_into(black_box(&mut buf)).unwrap();
            buf[hlen..hlen + size].copy_from_slice(black_box(&payload));
            black_box(hlen + size)
        })
    });

    group.finish();
}

fn bench_streaming_and_resync(c: &mut Criterion) {
    let mut group = c.benchmark_group("Framing/Streaming_and_Resync");

    // 1. FrameReader Multi-Frame Streaming
    let payload = vec![0x42u8; 256];
    let header = FrameHeader {
        profile: OptimizationProfile::Realtime,
        flags: FrameFlags::default(),
        payload_len: payload.len(),
        schema_hash: None,
    };
    let mut multi_buf = vec![0u8; (payload.len() + 32) * 16];
    let mut offset = 0;
    for _ in 0..16 {
        offset += pack_frame(&header, &payload, &mut multi_buf[offset..]).unwrap();
    }
    let multi_stream = &multi_buf[..offset];

    group.throughput(Throughput::Bytes(offset as u64));
    group.bench_function("FrameReader/16_frames_stream_4KiB", |b| {
        b.iter(|| {
            let mut reader = FrameReader::new(black_box(multi_stream));
            let mut count = 0;
            while let Ok(Some(frame)) = reader.next_frame() {
                black_box(frame.payload);
                count += 1;
            }
            assert_eq!(count, 16);
        })
    });

    // 2. find_magic AVX2 throughput (64 KiB buffer without magic)
    let clean_buf = vec![0x11u8; 65536];
    group.throughput(Throughput::Bytes(65536));
    group.bench_function("find_magic/AVX2_clean_64KiB", |b| {
        b.iter(|| find_magic(black_box(&clean_buf)))
    });

    // 3. find_magic SWAR throughput (64 KiB buffer without magic)
    group.bench_function("find_magic/SWAR_clean_64KiB", |b| {
        b.iter(|| find_magic_swar(black_box(&clean_buf)))
    });

    // 4. Stream Resync recovery after 256 bytes of noise
    let mut noisy_stream = vec![0xAAu8; 256 + payload.len() + 32];
    let frame_len = pack_frame(&header, &payload, &mut noisy_stream[256..]).unwrap();
    let stream_slice = &noisy_stream[..256 + frame_len];

    group.bench_function("resync_frame/after_256B_noise", |b| {
        b.iter(|| {
            let (skipped, frame) = resync_frame(black_box(stream_slice)).unwrap();
            black_box((skipped, frame.payload));
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_frame_pack_unpack,
    bench_frame_variations,
    bench_frame_crc_breakdown,
    bench_streaming_and_resync
);
criterion_main!(benches);
