//! Monotone sequence delta encoding and decoding.
//!
//! Provides utilities to delta-encode sequential integers relative to an explicit baseline
//! or relative to the preceding item.

use cbp_core::error::CbpError;

/// Applies delta encoding in-place on a slice of unsigned 32-bit integers,
/// with deltas relative to `slice[0]`: `[x0, x1 - x0, x2 - x1, ...]`.
/// The baseline `slice[0]` is retained as an absolute value.
#[inline]
pub fn delta_encode_u32(slice: &mut [u32]) {
    if slice.len() <= 1 {
        return;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if crate::simd::has_avx2() && slice.len() >= 8 {
            // SAFETY: We verified has_avx2() at runtime and slice.len() >= 8.
            unsafe {
                crate::simd::x86::delta_encode_u32_avx2(slice);
            }
            return;
        }
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

    #[cfg(target_arch = "x86_64")]
    {
        if crate::simd::has_avx2() && slice.len() >= 8 {
            // SAFETY: We verified has_avx2() at runtime and slice.len() >= 8.
            unsafe {
                crate::simd::x86::delta_decode_u32_avx2(slice);
            }
            return;
        }
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
pub fn delta_encode_with_base_u32(
    src: &[u32],
    baseline: u32,
    dst: &mut [u32],
) -> Result<(), CbpError> {
    if dst.len() < src.len() {
        return Err(CbpError::UnexpectedEof {
            required: src.len(),
            available: dst.len(),
        });
    }
    if src.is_empty() {
        return Ok(());
    }
    dst[..src.len()].copy_from_slice(src);
    delta_encode_u32(&mut dst[..src.len()]);
    dst[0] = src[0].wrapping_sub(baseline);
    Ok(())
}

/// Inverts delta encoding relative to an external baseline.
#[inline]
pub fn delta_decode_with_base_u32(
    src: &[u32],
    baseline: u32,
    dst: &mut [u32],
) -> Result<(), CbpError> {
    if dst.len() < src.len() {
        return Err(CbpError::UnexpectedEof {
            required: src.len(),
            available: dst.len(),
        });
    }
    if src.is_empty() {
        return Ok(());
    }
    dst[..src.len()].copy_from_slice(src);
    dst[0] = baseline.wrapping_add(src[0]);
    delta_decode_u32(&mut dst[..src.len()]);
    Ok(())
}

/// Stateless transformer for delta encoding.
pub struct DeltaTransformer;

impl DeltaTransformer {
    /// Encodes delta sequence into a separate destination slice.
    #[inline]
    pub fn encode(src: &[u32], dst: &mut [u32]) -> Result<(), CbpError> {
        if dst.len() < src.len() {
            return Err(CbpError::UnexpectedEof {
                required: src.len(),
                available: dst.len(),
            });
        }
        dst[..src.len()].copy_from_slice(src);
        delta_encode_u32(&mut dst[..src.len()]);
        Ok(())
    }

    /// Decodes delta sequence into a separate destination slice.
    #[inline]
    pub fn decode(src: &[u32], dst: &mut [u32]) -> Result<(), CbpError> {
        if dst.len() < src.len() {
            return Err(CbpError::UnexpectedEof {
                required: src.len(),
                available: dst.len(),
            });
        }
        dst[..src.len()].copy_from_slice(src);
        delta_decode_u32(&mut dst[..src.len()]);
        Ok(())
    }
}

/// Applies delta encoding in-place on a slice of signed 32-bit integers.
#[inline]
pub fn delta_encode_i32(slice: &mut [i32]) {
    if slice.len() <= 1 {
        return;
    }
    // SAFETY: i32 and u32 have identical memory layout, size (4 bytes), alignment (4 bytes),
    // and wrapping two's complement subtract bit-behavior.
    let u_slice =
        unsafe { core::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u32, slice.len()) };
    delta_encode_u32(u_slice);
}

/// Inverts delta encoding in-place on a slice of signed 32-bit integers.
#[inline]
pub fn delta_decode_i32(slice: &mut [i32]) {
    if slice.len() <= 1 {
        return;
    }
    // SAFETY: i32 and u32 have identical memory layout, size (4 bytes), alignment (4 bytes),
    // and wrapping two's complement add bit-behavior.
    let u_slice =
        unsafe { core::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u32, slice.len()) };
    delta_decode_u32(u_slice);
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
        delta_encode_with_base_u32(&original, baseline, &mut encoded).unwrap();
        assert_eq!(encoded, [2, 3, 2, 3]);

        let mut decoded = [0u32; 4];
        delta_decode_with_base_u32(&encoded, baseline, &mut decoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_delta_i32_roundtrip_negative_and_extrema() {
        let original = [-100i32, -98, -102, 0, 50, -50, i32::MIN, i32::MAX, 0];
        let mut encoded = original;
        delta_encode_i32(&mut encoded);

        let mut decoded = encoded;
        delta_decode_i32(&mut decoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_delta_simd_parity_various_lengths() {
        for len in [
            1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 128, 257, 1024,
        ] {
            let mut original = [0u32; 1024];
            for (i, val) in original[..len].iter_mut().enumerate() {
                *val = (i as u32) * 100 + ((i as u32 * 17) % 50);
            }

            let mut encoded = [0u32; 1024];
            encoded[..len].copy_from_slice(&original[..len]);
            delta_encode_u32(&mut encoded[..len]);

            // Scalar verification check
            assert_eq!(encoded[0], original[0], "len {}", len);
            for i in 1..len {
                assert_eq!(
                    encoded[i],
                    original[i].wrapping_sub(original[i - 1]),
                    "encode error at i={}, len={}",
                    i,
                    len
                );
            }

            let mut decoded = [0u32; 1024];
            decoded[..len].copy_from_slice(&encoded[..len]);
            delta_decode_u32(&mut decoded[..len]);

            assert_eq!(
                &decoded[..len],
                &original[..len],
                "decode error at len={}",
                len
            );
        }
    }
}
