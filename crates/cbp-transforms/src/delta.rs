//! Monotone sequence delta encoding and decoding.
//!
//! Provides utilities to delta-encode sequential integers relative to an explicit baseline
//! or relative to the preceding item.

/// Applies delta encoding in-place on a slice of unsigned 32-bit integers,
/// with deltas relative to `slice[0]`: `[x0, x1 - x0, x2 - x1, ...]`.
/// The baseline `slice[0]` is retained as an absolute value.
#[inline]
pub fn delta_encode_u32(slice: &mut [u32]) {
    if slice.len() <= 1 {
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
    if slice.len() <= 1 {
        return;
    }
    let mut prev = slice[0];
    for item in &mut slice[1..] {
        let restored = prev.wrapping_add(*item);
        *item = restored;
        prev = restored;
    }
}

/// Applies delta encoding where all elements (including the first) are relative to an external baseline:
/// `dst[i] = src[i] - baseline`.
#[inline]
pub fn delta_encode_with_base_u32(src: &[u32], baseline: u32, dst: &mut [u32]) {
    assert!(dst.len() >= src.len());
    let mut prev = baseline;
    for (i, &val) in src.iter().enumerate() {
        dst[i] = val.wrapping_sub(prev);
        prev = val;
    }
}

/// Inverts delta encoding relative to an external baseline.
#[inline]
pub fn delta_decode_with_base_u32(src: &[u32], baseline: u32, dst: &mut [u32]) {
    assert!(dst.len() >= src.len());
    let mut prev = baseline;
    for (i, &delta) in src.iter().enumerate() {
        let restored = prev.wrapping_add(delta);
        dst[i] = restored;
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

    #[test]
    fn test_delta_with_base_roundtrip() {
        let original = [1002u32, 1005, 1007, 1010];
        let baseline = 1000u32;

        let mut encoded = [0u32; 4];
        delta_encode_with_base_u32(&original, baseline, &mut encoded);
        assert_eq!(encoded, [2, 3, 2, 3]);

        let mut decoded = [0u32; 4];
        delta_decode_with_base_u32(&encoded, baseline, &mut decoded);
        assert_eq!(decoded, original);
    }
}
