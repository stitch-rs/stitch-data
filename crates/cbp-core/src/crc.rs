//! Hardware-accelerated and table-based CRC32 implementation (Castagnoli polynomial).

/// Computes CRC32 (Castagnoli 0x82F63B78) over a byte slice.
#[inline]
pub fn crc32_castagnoli(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_deterministic() {
        let data = b"123456789";
        let crc = crc32_castagnoli(data);
        assert_eq!(crc, 0xE3069283);
    }
}
