//! Integration tests for True Columnar (Structure-of-Arrays / SoA) Block Compression.

use stitch_data::prelude::*;

#[derive(Record, Debug, PartialEq, Clone, Copy, Default)]
struct TelemetryRecord {
    #[stitch(codec = "varint")]
    pub timestamp_ms: u32,
    #[stitch(codec = "zigzag")]
    pub temperature_c: i32,
    pub voltage: f32,
    pub alarm: bool,
}

#[test]
fn test_columnar_soa_batch_roundtrip_and_compression_gain() {
    let mut records = [TelemetryRecord::default(); 64];

    // Correlated sensor telemetry stream:
    // Monotonically increasing timestamps, smoothly varying temperatures, steady voltage
    for (i, r) in records.iter_mut().enumerate() {
        r.timestamp_ms = 1000 + (i as u32) * 10;
        r.temperature_c = 25 + (i as i32 % 3);
        r.voltage = 3.30 + (i as f32) * 0.001;
        r.alarm = i > 60;
    }

    // 1. Row-oriented AoS batch encoding
    let mut aos_buf = [0u8; 2048];
    let aos_len =
        TelemetryRecord::encode_batch(&records, &mut aos_buf).expect("AoS encode must succeed");

    // 2. Columnar SoA batch encoding
    let mut soa_buf = [0u8; 2048];
    let soa_len = TelemetryRecord::encode_columnar_batch(&records, &mut soa_buf)
        .expect("SoA encode must succeed");

    println!(
        "Batch of 64 records -> AoS: {} bytes | SoA Columnar: {} bytes",
        aos_len, soa_len
    );

    // Columnar SoA should be significantly more compact than row-oriented
    assert!(
        soa_len < aos_len,
        "SoA ({}) must be smaller than AoS ({})",
        soa_len,
        aos_len
    );

    // 3. Decode Columnar SoA batch back
    let mut decoded = [TelemetryRecord::default(); 64];
    let decoded_count = TelemetryRecord::decode_columnar_batch(&soa_buf[..soa_len], &mut decoded)
        .expect("SoA decode must succeed");

    assert_eq!(decoded_count, 64);
    assert_eq!(decoded, records);
}

#[test]
fn test_columnar_builder_flush_and_frame_roundtrip() {
    let mut builder =
        ColumnarBlockBuilder::<TelemetryRecord, 32>::new(OptimizationProfile::MaxDensity);

    for i in 0..32 {
        builder
            .push(TelemetryRecord {
                timestamp_ms: 10_000 + i * 5,
                temperature_c: 40 - (i as i32 % 2),
                voltage: 12.0,
                alarm: false,
            })
            .unwrap();
    }

    let mut scratch = [0u8; 1024];
    let mut frame_buf = [0u8; 2048];

    // Flush using columnar SoA block builder
    let frame_len = builder
        .flush(&mut scratch, &mut frame_buf)
        .expect("Flush must succeed");
    assert!(builder.is_empty());

    // Decode from framed packet
    let mut decoded = [TelemetryRecord::default(); 32];
    let count = decode_columnar_frame::<TelemetryRecord>(&frame_buf[..frame_len], &mut decoded)
        .expect("decode_columnar_frame must succeed");

    assert_eq!(count, 32);
    assert_eq!(decoded[0].timestamp_ms, 10_000);
    assert_eq!(decoded[31].timestamp_ms, 10_000 + 31 * 5);
}

#[test]
fn test_columnar_multi_chunk_boundaries() {
    // Test batch crossing the 256-record L1-cache chunk boundary (300 records)
    let mut records = [TelemetryRecord::default(); 300];
    for (i, r) in records.iter_mut().enumerate() {
        r.timestamp_ms = i as u32;
        r.temperature_c = (i as i32) - 150;
        r.voltage = 5.0;
        r.alarm = i % 2 == 0;
    }

    let mut soa_buf = [0u8; 8192];
    let soa_len = TelemetryRecord::encode_columnar_batch(&records, &mut soa_buf)
        .expect("Multi-chunk SoA encode must succeed");

    let mut decoded = [TelemetryRecord::default(); 300];
    let decoded_count = TelemetryRecord::decode_columnar_batch(&soa_buf[..soa_len], &mut decoded)
        .expect("Multi-chunk SoA decode must succeed");

    assert_eq!(decoded_count, 300);
    assert_eq!(decoded, records);
}

#[test]
fn test_columnar_empty_and_single_record() {
    let empty: [TelemetryRecord; 0] = [];
    let mut buf = [0u8; 64];
    let empty_len = TelemetryRecord::encode_columnar_batch(&empty, &mut buf).unwrap();
    assert_eq!(empty_len, 0);

    let mut empty_decoded: [TelemetryRecord; 0] = [];
    let dec_count =
        TelemetryRecord::decode_columnar_batch(&buf[..empty_len], &mut empty_decoded).unwrap();
    assert_eq!(dec_count, 0);

    let single = [TelemetryRecord {
        timestamp_ms: 999,
        temperature_c: -10,
        voltage: 1.5,
        alarm: true,
    }];
    let single_len = TelemetryRecord::encode_columnar_batch(&single, &mut buf).unwrap();
    assert!(single_len > 0);

    let mut single_decoded = [TelemetryRecord::default(); 1];
    let dec_single_count =
        TelemetryRecord::decode_columnar_batch(&buf[..single_len], &mut single_decoded).unwrap();
    assert_eq!(dec_single_count, 1);
    assert_eq!(single_decoded, single);
}

#[test]
fn test_columnar_dict_and_rle_codecs() {
    // 1. Test Dict column codec
    let categories = [10u32, 20, 10, 30, 20, 10, 10, 30, 40, 10];
    let mut dict_buf = [0u8; 128];
    let dict_len = encode_dict_column(&categories, &mut dict_buf).unwrap();

    let mut decoded_categories = [0u32; 10];
    let dec_dict_count =
        decode_dict_column(&dict_buf[..dict_len], &mut decoded_categories).unwrap();
    assert_eq!(dec_dict_count, 10);
    assert_eq!(decoded_categories, categories);

    // 2. Test RLE column codec
    let repeated = [7u32, 7, 7, 7, 9, 9, 9, 9, 9, 9, 12, 12];
    let mut rle_buf = [0u8; 128];
    let rle_len = encode_rle_column(&repeated, &mut rle_buf).unwrap();

    let mut decoded_repeated = [0u32; 12];
    let dec_rle_count = decode_rle_column(&rle_buf[..rle_len], &mut decoded_repeated).unwrap();
    assert_eq!(dec_rle_count, 12);
    assert_eq!(decoded_repeated, repeated);
}

#[derive(Debug, PartialEq, Clone, Default, Record)]
struct AdvancedTelemetryBatchRecord {
    #[stitch(column = "rle")]
    pub state_flags: u32,
    #[stitch(column = "dict")]
    pub category_id: u32,
    #[stitch(column = "gorilla")]
    pub sensor_reading: f32,
}

#[test]
fn test_derive_record_with_explicit_columnar_attributes() {
    let mut batch = Vec::new();
    for i in 0..100 {
        batch.push(AdvancedTelemetryBatchRecord {
            state_flags: if i < 50 { 1 } else { 2 },
            category_id: (i % 3) * 10,
            sensor_reading: 24.5 + (i as f32 * 0.1),
        });
    }

    let mut buf = [0u8; 4096];
    let written = AdvancedTelemetryBatchRecord::encode_columnar_batch(&batch, &mut buf).unwrap();
    assert!(written > 0);
    // Raw AoS would be 100 * (4 + 4 + 4) = 1200 bytes. Columnar RLE+Dict+Gorilla is significantly more compact.
    assert!(written < 600);

    let mut decoded = vec![AdvancedTelemetryBatchRecord::default(); 100];
    let count =
        AdvancedTelemetryBatchRecord::decode_columnar_batch(&buf[..written], &mut decoded).unwrap();
    assert_eq!(count, 100);
    assert_eq!(batch, decoded);
}
