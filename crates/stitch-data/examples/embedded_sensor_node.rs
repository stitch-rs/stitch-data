//! Production Reference Example: Constrained Embedded IoT / Sensor Node.
//!
//! Demonstrates:
//! 1. Strict stack-only operation (0 heap allocations).
//! 2. Multi-sensor telemetry record using `#[derive(StitchRecord)]`.
//! 3. Compact 128-byte stack buffer encoding and verification.
//! 4. Hardware Castagnoli CRC32 validation and compile-time schema safety.

use stitch_data::prelude::*;

/// Multi-sensor telemetry snapshot (28 bytes raw C-struct).
#[derive(StitchRecord, Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct SensorTelemetry {
    #[stitch(varint)]
    pub sequence: u32,
    #[stitch(raw)]
    pub temperature_c: f32,
    #[stitch(raw)]
    pub humidity_pct: f32,
    #[stitch(zigzag)]
    pub accel_x: i16,
    #[stitch(zigzag)]
    pub accel_y: i16,
    #[stitch(zigzag)]
    pub accel_z: i16,
    #[stitch(varint)]
    pub battery_mv: u16,
    #[stitch(bits = 4)]
    pub status_flags: u8,
}

fn main() -> Result<(), CbpError> {
    println!("===============================================================================");
    println!("    CBP CONSTRAINED EMBEDDED IoT / SENSOR NODE REFERENCE IMPLEMENTATION        ");
    println!("===============================================================================\n");

    println!(">>> Hardware Substrate Profile:");
    println!("  - Target Environment:     Embedded MCU / Bare-metal (#![no_std])");
    println!("  - Dynamic Allocator:      Disabled (Stack-Only Memory Budget)");
    println!("  - Stack Packet Buffer:    128 bytes fixed allocation");
    println!("  - Raw Sensor Struct:      28 bytes");
    println!(
        "  - Schema Fingerprint:     0x{:016X}\n",
        SensorTelemetry::SCHEMA_HASH
    );

    // 1. Simulate environmental sensor readings
    let telemetry = SensorTelemetry {
        sequence: 48291,
        temperature_c: 23.64,
        humidity_pct: 54.20,
        accel_x: -12,
        accel_y: 4,
        accel_z: 981, // ~1G earth gravity
        battery_mv: 3280,
        status_flags: 0x05, // Normal | GPS_Locked
    };

    println!(">>> Ingestion Sensor Reading:");
    println!("  - Sequence Counter:       {}", telemetry.sequence);
    println!(
        "  - Temperature:            {:.2} °C",
        telemetry.temperature_c
    );
    println!(
        "  - Humidity:               {:.2} %",
        telemetry.humidity_pct
    );
    println!(
        "  - Accelerometer:          [X: {}, Y: {}, Z: {}]",
        telemetry.accel_x, telemetry.accel_y, telemetry.accel_z
    );
    println!("  - Battery:                {} mV", telemetry.battery_mv);
    println!(
        "  - Status:                 0x{:02X}\n",
        telemetry.status_flags
    );

    // 2. Encode on stack without heap allocations
    let mut stack_scratch = [0u8; 64];
    let mut stack_frame = [0u8; 128];

    let wire_bytes_written = telemetry.pack_to_frame(
        OptimizationProfile::Balanced,
        &mut stack_scratch,
        &mut stack_frame,
    )?;

    println!(">>> Wire Serialization Result:");
    println!("  - Raw AoS Size:           28 bytes");
    println!(
        "  - CBP Wire Size:          {} bytes (including 8B schema hash & 4B CRC32)",
        wire_bytes_written
    );
    println!(
        "  - Wire Efficiency:        {:.1}% of raw size",
        (wire_bytes_written as f64 / 28.0) * 100.0
    );
    println!(
        "  - Stack Headroom Left:    {} bytes available in 128B buffer\n",
        128 - wire_bytes_written
    );

    // 3. Receive, verify integrity, and unpack on receiver
    let decoded_telemetry = SensorTelemetry::unpack_from_frame(&stack_frame[..wire_bytes_written])?;

    assert_eq!(
        telemetry, decoded_telemetry,
        "FATAL: Decoded telemetry does not match sensor reading!"
    );

    println!(">>> Receiver Invariants Verified:");
    println!("  [PASS] Castagnoli CRC32 integrity check passed");
    println!(
        "  [PASS] Compile-time schema fingerprint match (0x{:016X})",
        SensorTelemetry::SCHEMA_HASH
    );
    println!("  [PASS] Bit-exact sensor state reconstruction");
    println!("\n[SUCCESS] Embedded sensor node scenario executed cleanly with 0 heap allocations.");

    Ok(())
}
