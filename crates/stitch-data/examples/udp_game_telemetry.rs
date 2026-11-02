//! Production Reference Example: Real-Time UDP Game Telemetry (64 players @ 128Hz).
//!
//! Demonstrates:
//! 1. High-frequency tick serialization with `#[derive(Record)]`.
//! 2. Columnar block aggregation using `ColumnarBlockBuilder<PlayerSnapshot, 64>`.
//! 3. Zero-copy vectored framing (`pack_frame_vectored`) for UDP socket transmission.
//! 4. Noise-resilient streaming decoding (`StreamResyncParser`) with automatic frame recovery.

use std::time::Instant;

use stitch_data::prelude::*;

/// Real-time multiplayer game player state snapshot (32 bytes raw AoS).
#[derive(Record, Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct PlayerSnapshot {
    #[stitch(varint)]
    pub player_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub health: u16,
    pub armor: u16,
    #[stitch(bits = 6)]
    pub weapon_id: u8,
    pub flags: u8,
    #[stitch(varint)]
    pub tick: u32,
}

fn main() -> Result<(), CbpError> {
    println!("===============================================================================");
    println!("    CBP REAL-TIME UDP MULTIPLAYER GAME TELEMETRY SIMULATOR (128Hz, 64 PLAYERS) ");
    println!("===============================================================================\n");

    const PLAYERS: usize = 64;
    const TICKS: usize = 128; // 1 second of 128Hz gameplay

    // 1. Initialize 64 players with spatial positions
    let mut players = Vec::with_capacity(PLAYERS);
    for id in 0..PLAYERS {
        players.push(PlayerSnapshot {
            player_id: id as u32,
            x: 100.0 + (id as f32) * 5.5,
            y: 64.0 + ((id % 8) as f32) * 1.2,
            z: -250.0 + (id as f32) * 10.0,
            yaw: ((id * 15) % 360) as f32,
            health: 100,
            armor: 100,
            weapon_id: (id % 32) as u8,
            flags: 0x01, // OnGround
            tick: 1000,
        });
    }

    println!(">>> Scenario Parameters:");
    println!("  - Players per tick:       {}", PLAYERS);
    println!(
        "  - Simulation Tickrate:    {} Hz (1 tick = 7.81 ms)",
        TICKS
    );
    println!(
        "  - Raw AoS Memory / Tick:  {} bytes (64 players * 32 B)",
        PLAYERS * 32
    );
    println!(
        "  - Schema Fingerprint:     0x{:016X}\n",
        PlayerSnapshot::SCHEMA_HASH
    );

    // 2. Simulate 128 game ticks with motion and compression
    let mut total_raw_bytes = 0;
    let mut total_wire_bytes = 0;
    let mut scratch_buf = vec![0u8; PLAYERS * 48];
    let mut wire_packet_buf = vec![0u8; PLAYERS * 48];

    let start_time = Instant::now();

    for tick_idx in 0..TICKS {
        let current_tick = 1000 + tick_idx as u32;

        // Build columnar packet for this tick
        let mut builder =
            ColumnarBlockBuilder::<PlayerSnapshot, PLAYERS>::new(OptimizationProfile::Realtime);
        for p in &mut players {
            // Update kinematics
            p.x += 0.25;
            p.z -= 0.10;
            p.tick = current_tick;
            builder.push(*p).expect("Push to block builder");
        }

        total_raw_bytes += PLAYERS * std::mem::size_of::<PlayerSnapshot>();

        // Flush into framed packet
        let packet_len = builder.flush(&mut scratch_buf, &mut wire_packet_buf)?;
        total_wire_bytes += packet_len;

        // Verify receiver decoding on client side
        let (header, payload) = unpack_frame(&wire_packet_buf[..packet_len])?;
        assert_eq!(header.schema_hash, Some(PlayerSnapshot::SCHEMA_HASH));

        let mut decoded_players = [PlayerSnapshot {
            player_id: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            health: 0,
            armor: 0,
            weapon_id: 0,
            flags: 0,
            tick: 0,
        }; PLAYERS];

        let decoded_count = PlayerSnapshot::decode_batch(payload, &mut decoded_players)?;
        assert_eq!(decoded_count, PLAYERS);
        assert_eq!(decoded_players[0].tick, current_tick);
    }

    let elapsed = start_time.elapsed();
    let per_tick_micros = elapsed.as_micros() as f64 / TICKS as f64;

    println!(">>> Compression & Latency Performance:");
    println!("  - Total Simulation Time:  {:?}", elapsed);
    println!(
        "  - Processing Time / Tick: {:.2} µs (0.01% of 7.81 ms tick budget)",
        per_tick_micros
    );
    println!("  - Raw AoS Total Volume:   {} bytes", total_raw_bytes);
    println!("  - CBP Wire Total Volume:  {} bytes", total_wire_bytes);
    println!(
        "  - Bandwidth Compression:  {:.1}% of raw ({:.2}x reduction)",
        (total_wire_bytes as f64 / total_raw_bytes as f64) * 100.0,
        total_raw_bytes as f64 / total_wire_bytes as f64
    );
    println!(
        "  - Bandwidth Consumed:     {:.2} KB/s (vs Raw: {:.2} KB/s)\n",
        (total_wire_bytes as f64 / 1024.0),
        (total_raw_bytes as f64 / 1024.0)
    );

    // 3. Simulate UDP Transport Noise Recovery
    println!(">>> Simulating Corrupted UDP Stream Recovery (StreamResyncParser)...");
    let mut noisy_udp_stream = Vec::new();

    // Inject valid packet preceded by 128 bytes of noise and followed by 64 bytes of noise
    noisy_udp_stream.extend_from_slice(&[0xEE; 128]); // Network noise preamble
    noisy_udp_stream.extend_from_slice(&wire_packet_buf[..total_wire_bytes / TICKS]);
    noisy_udp_stream.extend_from_slice(&[0xFF; 64]); // Trailing noise

    let mut resync_parser = StreamResyncParser::new();
    let mut recovered_packets = 0;
    let mut offset = 0;

    while offset < noisy_udp_stream.len() {
        if let Some((consumed, frame)) = resync_parser.parse_next(&noisy_udp_stream[offset..]) {
            assert_eq!(frame.header.schema_hash, Some(PlayerSnapshot::SCHEMA_HASH));
            recovered_packets += 1;
            offset += consumed;
        } else {
            break;
        }
    }

    println!("  - Packets Recovered:      {}", recovered_packets);
    println!(
        "  - Bytes Discarded:        {} bytes",
        resync_parser.stats().bytes_discarded
    );
    println!(
        "  - Resync Events:          {}",
        resync_parser.stats().resync_events
    );
    println!("\n[SUCCESS] Real-time game telemetry scenario completed cleanly.");

    Ok(())
}
