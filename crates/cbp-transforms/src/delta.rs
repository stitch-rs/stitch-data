//! Monotone sequence delta encoding and decoding.

/// Applies delta encoding in-place on a slice of unsigned 32-bit integers.
/// `[x0, x1, x2, ...] -> [x0, x1 - x0, x2 - x1, ...]`
#[inline]
pub fn delta_encode_u32(slice: &mut [u32]) {
    if slice.is_empty() {
        return;
    }
    let mut prev = slice[0];
    for item in &mut slice[1..] {
        let current = *item;
        *item = current.wrapping_sub(prev);
        prev = current;
    }
}

/// Inverts delta encoding in-place on a slice of unsigned 32-bit integers.
#[inline]
pub fn delta_decode_u32(slice: &mut [u32]) {
    if slice.is_empty() {
        return;
    }
    let mut prev = slice[0];
    for item in &mut slice[1..] {
        let restored = prev.wrapping_add(*item);
        *item = restored;
        prev = restored;
    }
}

/// Stateless transformer for delta encoding.
pub struct DeltaTransformer;

impl DeltaTransformer {
    /// Encodes delta sequence into a separate destination slice.
    #[inline]
    pub fn encode(src: &[u32], dst: &mut [u32]) {
        assert!(dst.len() >= src.len());
        dst[..src.len()].copy_from_slice(src);
        delta_encode_u32(&mut dst[..src.len()]);
    }

    /// Decodes delta sequence into a separate destination slice.
    #[inline]
    pub fn decode(src: &[u32], dst: &mut [u32]) {
        assert!(dst.len() >= src.len());
        dst[..src.len()].copy_from_slice(src);
        delta_decode_u32(&mut dst[..src.len()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_roundtrip() {
        let original = [1000u32, 1004, 1010, 1050, 2000, 2001];
        let mut encoded = original;
        delta_encode_u32(&mut encoded);

        assert_eq!(encoded, [1000, 4, 6, 40, 950, 1]);

        let mut decoded = encoded;
        delta_decode_u32(&mut decoded);
        assert_eq!(decoded, original);
    }
}
