//! High-volume deterministic adversarial fuzzing test suite.
//!
//! Executes 100,000+ adversarial iterations including truncation attacks,
//! bit-flip injections, schema spoofing, and stream corruption.

use cbp::prelude::*;
use cbp_fuzz::*;

/// Minimal Lehmer LCG for deterministic pseudo-random fuzz payload generation.
struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xdeadbeefcafe1337 } else { seed },
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }

    fn fill_bytes(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            let r = self.next_u32();
            let bytes = r.to_le_bytes();
            let take = (buf.len() - i).min(4);
            buf[i..i + take].copy_from_slice(&bytes[..take]);
            i += take;
        }
    }
}

#[test]
fn test_adversarial_random_bytes_never_panic() {
    let mut rng = FuzzRng::new(42);
    let mut buf = [0u8; 512];

    for _ in 0..100_000 {
        let len = (rng.next_u32() as usize % buf.len()) + 1;
        rng.fill_bytes(&mut buf[..len]);
        let slice = &buf[..len];

        // All entrypoints must handle arbitrary bytes without panics
        fuzz_unpack_frame(slice);
        fuzz_frame_reader(slice);
        fuzz_prefix_varint(slice);
        fuzz_bitpack(slice);
        fuzz_gorilla_float(slice);
        fuzz_derived_record(slice);
    }
}

#[test]
fn test_adversarial_truncation_attacks_on_valid_frames() {
    let record = FuzzRecord {
        id: 1042,
        delta_x: -55,
        coord_y: 12.345,
        tag: 15,
        is_valid: true,
    };

    let mut scratch = [0u8; 64];
    let mut frame_buf = [0u8; 64];
    let frame_len = record
        .pack_to_frame(OptimizationProfile::Balanced, &mut scratch, &mut frame_buf)
        .expect("Valid pack must succeed");

    assert!(frame_len >= 12);

    // Truncate at every possible length from 0 to frame_len - 1
    for trunc_len in 0..frame_len {
        let truncated = &frame_buf[..trunc_len];
        let result = unpack_frame(truncated);
        assert!(
            result.is_err(),
            "Truncated frame of len {trunc_len} must return Err, got Ok"
        );
        match result.unwrap_err() {
            CbpError::UnexpectedEof { .. } => {}
            other => panic!("Expected UnexpectedEof on truncated frame, got: {other:?}"),
        }
    }
}

#[test]
fn test_adversarial_bit_flip_crc_rejection() {
    let record = FuzzRecord {
        id: 99999,
        delta_x: 123456,
        coord_y: 987.654,
        tag: 33,
        is_valid: false,
    };

    let mut scratch = [0u8; 64];
    let mut frame_buf = [0u8; 64];
    let frame_len = record
        .pack_to_frame(OptimizationProfile::Direct, &mut scratch, &mut frame_buf)
        .expect("Valid pack must succeed");

    // Invert every single bit in the frame one by one
    for byte_idx in 0..frame_len {
        for bit_idx in 0..8 {
            let mut corrupted = frame_buf;
            corrupted[byte_idx] ^= 1 << bit_idx;

            let result = unpack_frame(&corrupted[..frame_len]);
            assert!(
                result.is_err(),
                "Corrupted frame (byte {byte_idx}, bit {bit_idx}) must fail"
            );
            // It must be either ChecksumMismatch, InvalidMagic, InvalidHeaderFlags, FrameTooLarge or UnexpectedEof
            match result.unwrap_err() {
                CbpError::ChecksumMismatch { .. }
                | CbpError::InvalidMagic(_)
                | CbpError::UnsupportedVersion(_)
                | CbpError::InvalidHeaderFlags(_)
                | CbpError::FrameTooLarge { .. }
                | CbpError::UnexpectedEof { .. }
                | CbpError::MalformedVarint => {}
                other => panic!("Unexpected error on flipped bit: {other:?}"),
            }
        }
    }
}

#[test]
fn test_adversarial_huge_payload_length_dos_defense() {
    let mut fake_frame = [0u8; 32];
    fake_frame[0] = CBP_MAGIC; // 0xCB
    fake_frame[1] = CBP_VERSION_1; // 0x01
    fake_frame[2] = 0x00; // Profile Direct, no flags

    // Encode huge payload length (e.g. 1 GiB) via PrefixVarint
    let vlen = encode_prefix_varint(1 << 30, &mut fake_frame[3..]);
    assert!(vlen > 0);

    // Unpack must immediately return FrameTooLarge or UnexpectedEof without allocating memory
    let res = unpack_frame(&fake_frame);
    assert!(res.is_err());
    match res.unwrap_err() {
        CbpError::FrameTooLarge { size, limit } => {
            assert_eq!(size, 1 << 30);
            assert_eq!(limit, 64 * 1024 * 1024);
        }
        CbpError::UnexpectedEof {
            required,
            available,
        } => {
            assert!(required > 1 << 30);
            assert_eq!(available, fake_frame.len());
        }
        other => panic!("Expected FrameTooLarge or UnexpectedEof, got: {other:?}"),
    }
}

#[test]
fn test_adversarial_schema_hash_spoofing() {
    let record = FuzzRecord {
        id: 77,
        delta_x: 0,
        coord_y: 0.0,
        tag: 1,
        is_valid: true,
    };

    let mut scratch = [0u8; 64];
    let mut frame_buf = [0u8; 64];
    let frame_len = record
        .pack_to_frame(OptimizationProfile::Balanced, &mut scratch, &mut frame_buf)
        .expect("Pack must succeed");

    // Corrupt the schema hash inside the frame header
    // Header format: [0xCB, 0x01, ProfileFlags(has_schema=true), VarintLen, SchemaHash(8B)...]
    // Find schema hash offset after varint:
    let (_vlen_val, vlen_bytes) = decode_prefix_varint(&frame_buf[3..]).unwrap();
    let schema_offset = 3 + vlen_bytes;

    // Tamper with schema hash
    let mut tampered_frame = frame_buf;
    tampered_frame[schema_offset] ^= 0xFF;

    // Recalculate CRC32 so frame passes CRC but fails schema validation
    let _payload_len = frame_len - 4 - schema_offset - 8;
    let header_and_payload_len = frame_len - 4;
    let new_crc = crc32_castagnoli(&tampered_frame[..header_and_payload_len]);
    tampered_frame[frame_len - 4..frame_len].copy_from_slice(&new_crc.to_le_bytes());

    // Unpack must fail fast with SchemaMismatch
    let res = FuzzRecord::unpack_from_frame(&tampered_frame[..frame_len]);
    assert!(res.is_err());
    match res.unwrap_err() {
        CbpError::SchemaMismatch { expected, actual } => {
            assert_eq!(expected, FuzzRecord::SCHEMA_HASH);
            assert_ne!(actual, expected);
        }
        other => panic!("Expected SchemaMismatch, got: {other:?}"),
    }
}

#[test]
fn test_adversarial_stream_resync_fuzzing() {
    let mut rng = FuzzRng::new(999);
    let mut stream = Vec::new();

    // Create 10 valid frames interspersed with random garbage noise
    let mut valid_records = Vec::new();
    for i in 0..10 {
        let record = FuzzRecord {
            id: 100 + i,
            delta_x: (i as i32) * -5,
            coord_y: 1.0 * (i as f32),
            tag: (i % 8) as u8,
            is_valid: i % 2 == 0,
        };
        valid_records.push(record);

        // Prepend 10-100 bytes of random garbage noise
        let noise_len = (rng.next_u32() as usize % 90) + 10;
        let mut noise = vec![0u8; noise_len];
        rng.fill_bytes(&mut noise);
        stream.extend_from_slice(&noise);

        // Append valid frame
        let mut scratch = [0u8; 64];
        let mut frame_buf = [0u8; 64];
        let frame_len = record
            .pack_to_frame(OptimizationProfile::Balanced, &mut scratch, &mut frame_buf)
            .unwrap();
        stream.extend_from_slice(&frame_buf[..frame_len]);
    }

    // Append trailing noise
    let mut trailing_noise = vec![0u8; 50];
    rng.fill_bytes(&mut trailing_noise);
    stream.extend_from_slice(&trailing_noise);

    // Parser must recover all 10 frames cleanly
    let mut parser = StreamResyncParser::new();
    let mut offset = 0;
    let mut recovered = Vec::new();
    while offset < stream.len() {
        if let Some((consumed, frame)) = parser.parse_next(&stream[offset..]) {
            let (rec, _) = FuzzRecord::decode_record(frame.payload).unwrap();
            recovered.push(rec);
            offset += consumed;
        } else {
            break;
        }
    }

    assert_eq!(recovered.len(), 10);
    assert_eq!(recovered, valid_records);
    assert!(parser.stats().bytes_discarded > 100);
}
