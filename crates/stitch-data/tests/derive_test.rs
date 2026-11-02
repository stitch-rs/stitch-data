use stitch_data::prelude::*;

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct TestTelemetry {
    #[stitch(varint)]
    pub sequence_id: u32,
    #[stitch(zigzag)]
    pub offset_x: i32,
    #[stitch(raw)]
    pub velocity: f32,
    #[stitch(bits = 8)]
    pub status_code: u8,
    pub is_active: bool,
}

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct IncompatibleTelemetry {
    pub sequence_id: u64, // Different type!
    pub is_active: bool,
}

#[test]
fn test_derive_record_roundtrip() {
    let record = TestTelemetry {
        sequence_id: 1042,
        offset_x: -250,
        velocity: 123.456,
        status_code: 7,
        is_active: true,
    };

    let mut buf = [0u8; 128];
    let written = record.encode_record(&mut buf).expect("encode must succeed");
    assert!(written > 0);

    let (decoded, consumed) =
        TestTelemetry::decode_record(&buf[..written]).expect("decode must succeed");
    assert_eq!(consumed, written);
    assert_eq!(decoded, record);
}

#[test]
fn test_derive_record_framed_roundtrip_and_schema_validation() {
    let record = TestTelemetry {
        sequence_id: 2048,
        offset_x: 500,
        velocity: -98.76,
        status_code: 15,
        is_active: false,
    };

    let mut scratch = [0u8; 128];
    let mut framed_dst = [0u8; 256];

    let total = record
        .pack_to_frame(OptimizationProfile::Balanced, &mut scratch, &mut framed_dst)
        .expect("pack_to_frame must succeed");

    let unpacked =
        TestTelemetry::unpack_from_frame(&framed_dst[..total]).expect("unpack must succeed");
    assert_eq!(unpacked, record);

    // Test Schema Mismatch rejection: unpacking as IncompatibleTelemetry must fail with SchemaMismatch!
    let err = IncompatibleTelemetry::unpack_from_frame(&framed_dst[..total]).unwrap_err();
    match err {
        CbpError::SchemaMismatch { expected, actual } => {
            assert_eq!(expected, IncompatibleTelemetry::SCHEMA_HASH);
            assert_eq!(actual, TestTelemetry::SCHEMA_HASH);
        }
        other => panic!("expected SchemaMismatch, got {:?}", other),
    }
}

#[test]
fn test_derive_record_with_columnar_builder() {
    let mut builder = ColumnarBlockBuilder::<TestTelemetry, 8>::new(OptimizationProfile::Realtime);

    for i in 0..8 {
        let is_full = builder
            .push(TestTelemetry {
                sequence_id: 1000 + i as u32,
                offset_x: i * -10,
                velocity: 1.0 * (i as f32),
                status_code: (i % 4) as u8,
                is_active: i % 2 == 0,
            })
            .expect("push must succeed");

        if i == 7 {
            assert!(is_full);
        } else {
            assert!(!is_full);
        }
    }

    let mut scratch = [0u8; 512];
    let mut packet = [0u8; 1024];

    let packet_len = builder
        .flush(&mut scratch, &mut packet)
        .expect("flush must succeed");
    assert!(packet_len > 0);

    // Unpack frame
    let (header, payload) = unpack_frame(&packet[..packet_len]).expect("unpack_frame must succeed");
    assert_eq!(header.schema_hash, Some(TestTelemetry::SCHEMA_HASH));
    assert!(header.flags.is_chunked);

    let mut records_out = [TestTelemetry {
        sequence_id: 0,
        offset_x: 0,
        velocity: 0.0,
        status_code: 0,
        is_active: false,
    }; 8];

    let decoded_count =
        TestTelemetry::decode_batch(payload, &mut records_out).expect("decode_batch must succeed");
    assert_eq!(decoded_count, 8);
    assert_eq!(records_out[0].sequence_id, 1000);
    assert_eq!(records_out[0].offset_x, 0);
    assert!(records_out[0].is_active);
    assert_eq!(records_out[7].sequence_id, 1007);
    assert_eq!(records_out[7].offset_x, -70);
    assert!(!records_out[7].is_active);
}

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct BoundedRecord {
    #[stitch(bits = 4)]
    pub nibble: u8,
}

#[test]
fn test_bounded_bit_pack_rejects_out_of_range() {
    let invalid = BoundedRecord { nibble: 16 }; // > 15 (4 bits)
    let mut buf = [0u8; 16];
    let err = invalid.encode_record(&mut buf).unwrap_err();
    assert_eq!(err, CbpError::InvalidBitWidth(4));

    let valid = BoundedRecord { nibble: 15 };
    let written = valid.encode_record(&mut buf).unwrap();
    assert_eq!(written, 1);
    let (dec, consumed) = BoundedRecord::decode_record(&buf[..written]).unwrap();
    assert_eq!(consumed, 1);
    assert_eq!(dec.nibble, 15);
}

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct BitstreamPackedRecord {
    #[stitch(bits = 1)]
    pub flag: bool,
    #[stitch(bits = 3)]
    pub category: u8,
    #[stitch(bits = 12)]
    pub sensor_raw: u16,
}

#[test]
fn test_sub_byte_consecutive_bitstream_packing() {
    let rec = BitstreamPackedRecord {
        flag: true,
        category: 5,      // 3 bits (0..=7)
        sensor_raw: 4000, // 12 bits (0..=4095)
    };
    let mut buf = [0u8; 16];
    // 1 + 3 + 12 = 16 bits = exactly 2 bytes!
    let written = rec.encode_record(&mut buf).unwrap();
    assert_eq!(written, 2);

    let (decoded, consumed) = BitstreamPackedRecord::decode_record(&buf[..written]).unwrap();
    assert_eq!(consumed, 2);
    assert_eq!(decoded, rec);
}

#[test]
fn test_optimization_profile_direct_vs_balanced() {
    let record = TestTelemetry {
        sequence_id: 1042,
        offset_x: -250,
        velocity: 123.456,
        status_code: 7,
        is_active: true,
    };
    let mut buf_direct = [0u8; 64];
    let mut buf_balanced = [0u8; 64];

    let written_direct = record
        .encode_record_profile(OptimizationProfile::Direct, &mut buf_direct)
        .unwrap();
    let written_balanced = record
        .encode_record_profile(OptimizationProfile::Balanced, &mut buf_balanced)
        .unwrap();

    // Direct writes uncompressed POD: u32 (4) + i32 (4) + f32 (4) + u8 (1) + bool (1) = 14 bytes
    assert_eq!(written_direct, 14);

    let (dec_direct, cons_direct) = TestTelemetry::decode_record_profile(
        &buf_direct[..written_direct],
        OptimizationProfile::Direct,
    )
    .unwrap();
    assert_eq!(cons_direct, written_direct);
    assert_eq!(dec_direct, record);

    let (dec_balanced, cons_balanced) = TestTelemetry::decode_record_profile(
        &buf_balanced[..written_balanced],
        OptimizationProfile::Balanced,
    )
    .unwrap();
    assert_eq!(cons_balanced, written_balanced);
    assert_eq!(dec_balanced, record);
}

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct StructFieldOrderA {
    pub x: u32,
    pub y: u64,
}

#[derive(StitchRecord, Debug, PartialEq, Clone, Copy)]
struct StructFieldOrderB {
    pub y: u64,
    pub x: u32,
}

#[test]
fn test_schema_hash_differentiates_field_order_and_names() {
    assert_ne!(
        StructFieldOrderA::SCHEMA_HASH,
        StructFieldOrderB::SCHEMA_HASH
    );
}
