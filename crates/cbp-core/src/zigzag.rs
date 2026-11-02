//! Signed integer ZigZag encoding and decoding.
//!
//! Maps signed integers to positive unsigned integers, preserving magnitude
//! locality (e.g., -1 -> 1, 1 -> 2, -2 -> 3) so that small negative numbers
//! compress efficiently with VarInt.

/// Encodes an `i32` into a ZigZag-encoded `u32`.
#[inline(always)]
pub const fn encode_zigzag32(n: i32) -> u32 {
    ((n << 1) ^ (n >> 31)) as u32
}

/// Decodes a ZigZag-encoded `u32` back into an `i32`.
#[inline(always)]
pub const fn decode_zigzag32(n: u32) -> i32 {
    ((n >> 1) as i32) ^ (-((n & 1) as i32))
}

/// Encodes an `i64` into a ZigZag-encoded `u64`.
#[inline(always)]
pub const fn encode_zigzag64(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Decodes a ZigZag-encoded `u64` back into an `i64`.
#[inline(always)]
pub const fn decode_zigzag64(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zigzag32_roundtrip() {
        let cases = [0, -1, 1, -2, 2, i32::MIN, i32::MAX, -1000, 1000];
        for &val in &cases {
            assert_eq!(decode_zigzag32(encode_zigzag32(val)), val);
        }
    }

    #[test]
    fn test_zigzag64_roundtrip() {
        let cases = [0, -1, 1, -2, 2, i64::MIN, i64::MAX, -100_000, 100_000];
        for &val in &cases {
            assert_eq!(decode_zigzag64(encode_zigzag64(val)), val);
        }
    }
}
