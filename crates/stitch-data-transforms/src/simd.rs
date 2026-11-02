//! Hardware-accelerated SIMD kernels and CPU feature detection for `#![no_std]`.
//!
//! Provides AVX2/SSE-accelerated kernels for delta encoding/decoding and bit-packing
//! transformations with automatic CPU feature detection and safe zero-cost dispatch.

pub use stitch_data_core::simd::has_avx2;

#[cfg(target_arch = "x86_64")]
pub mod x86 {
    use core::arch::x86_64::*;

    /// Vectorized in-place delta encoding for `u32` slice using register shifts.
    ///
    /// # Safety
    /// Caller must ensure AVX2 target feature is supported on the host CPU.
    #[target_feature(enable = "avx2")]
    pub unsafe fn delta_encode_u32_avx2(slice: &mut [u32]) {
        if slice.len() <= 1 {
            return;
        }

        let len = slice.len();
        let ptr = slice.as_mut_ptr();
        let mut i = 1;
        // SAFETY: slice.len() > 1 verified above.
        let mut prev = unsafe { *ptr };

        // SAFETY: Pointer arithmetic and SIMD loads/stores are bounded within slice length.
        unsafe {
            // Process in 8-element unrolled blocks (two 128-bit vector registers)
            while i + 8 <= len {
                let r0 = _mm_loadu_si128(ptr.add(i) as *const __m128i);
                let p0 = _mm_or_si128(_mm_slli_si128(r0, 4), _mm_cvtsi32_si128(prev as i32));
                let d0 = _mm_sub_epi32(r0, p0);
                let mid_prev = _mm_extract_epi32(r0, 3) as u32;

                let r1 = _mm_loadu_si128(ptr.add(i + 4) as *const __m128i);
                let p1 = _mm_or_si128(_mm_slli_si128(r1, 4), _mm_cvtsi32_si128(mid_prev as i32));
                let d1 = _mm_sub_epi32(r1, p1);
                prev = _mm_extract_epi32(r1, 3) as u32;

                _mm_storeu_si128(ptr.add(i) as *mut __m128i, d0);
                _mm_storeu_si128(ptr.add(i + 4) as *mut __m128i, d1);

                i += 8;
            }

            // Process 4-element block
            while i + 4 <= len {
                let r0 = _mm_loadu_si128(ptr.add(i) as *const __m128i);
                let p0 = _mm_or_si128(_mm_slli_si128(r0, 4), _mm_cvtsi32_si128(prev as i32));
                let d0 = _mm_sub_epi32(r0, p0);
                prev = _mm_extract_epi32(r0, 3) as u32;

                _mm_storeu_si128(ptr.add(i) as *mut __m128i, d0);
                i += 4;
            }

            // Scalar remainder
            while i < len {
                let current = *ptr.add(i);
                *ptr.add(i) = current.wrapping_sub(prev);
                prev = current;
                i += 1;
            }
        }
    }

    /// AVX2/SSE-accelerated in-place delta decoding (prefix sum) for `u32` slice.
    ///
    /// # Safety
    /// Caller must ensure AVX2 target feature is supported on the host CPU.
    #[target_feature(enable = "avx2")]
    pub unsafe fn delta_decode_u32_avx2(slice: &mut [u32]) {
        if slice.len() <= 1 {
            return;
        }

        let len = slice.len();
        let ptr = slice.as_mut_ptr();
        let mut i = 1;
        // SAFETY: slice.len() > 1 verified above.
        let mut prev = unsafe { *ptr };

        // SAFETY: Pointer arithmetic and SIMD loads/stores are bounded within slice length.
        unsafe {
            // Process in 8-element blocks using 128-bit Kogge-Stone parallel scan cascade
            while i + 8 <= len {
                // Lower 4 elements (i .. i+4)
                let mut v0 = _mm_loadu_si128(ptr.add(i) as *const __m128i);
                let s1 = _mm_slli_si128(v0, 4);
                v0 = _mm_add_epi32(v0, s1);
                let s2 = _mm_slli_si128(v0, 8);
                v0 = _mm_add_epi32(v0, s2);
                v0 = _mm_add_epi32(v0, _mm_set1_epi32(prev as i32));
                _mm_storeu_si128(ptr.add(i) as *mut __m128i, v0);

                let mid_prev = _mm_extract_epi32(v0, 3) as u32;

                // Upper 4 elements (i+4 .. i+8)
                let mut v1 = _mm_loadu_si128(ptr.add(i + 4) as *const __m128i);
                let s3 = _mm_slli_si128(v1, 4);
                v1 = _mm_add_epi32(v1, s3);
                let s4 = _mm_slli_si128(v1, 8);
                v1 = _mm_add_epi32(v1, s4);
                v1 = _mm_add_epi32(v1, _mm_set1_epi32(mid_prev as i32));
                _mm_storeu_si128(ptr.add(i + 4) as *mut __m128i, v1);

                prev = _mm_extract_epi32(v1, 3) as u32;
                i += 8;
            }

            // Process remaining 4-element block
            while i + 4 <= len {
                let mut v0 = _mm_loadu_si128(ptr.add(i) as *const __m128i);
                let s1 = _mm_slli_si128(v0, 4);
                v0 = _mm_add_epi32(v0, s1);
                let s2 = _mm_slli_si128(v0, 8);
                v0 = _mm_add_epi32(v0, s2);
                v0 = _mm_add_epi32(v0, _mm_set1_epi32(prev as i32));
                _mm_storeu_si128(ptr.add(i) as *mut __m128i, v0);

                prev = _mm_extract_epi32(v0, 3) as u32;
                i += 4;
            }

            // Scalar remainder
            while i < len {
                let restored = prev.wrapping_add(*ptr.add(i));
                *ptr.add(i) = restored;
                prev = restored;
                i += 1;
            }
        }
    }

    /// AVX2-accelerated zero-extension unpacking of 8-bit integers to `u32`.
    ///
    /// # Safety
    /// Caller must ensure AVX2 target feature is supported on the host CPU.
    #[target_feature(enable = "avx2")]
    pub unsafe fn unpack_8bit_avx2(src: &[u8], dst: &mut [u32]) {
        let count = dst.len();
        let src_ptr = src.as_ptr();
        let dst_ptr = dst.as_mut_ptr();
        let mut i = 0;

        // SAFETY: Pointer arithmetic and vector loads/stores bounded by count.
        unsafe {
            // Process 16 elements (64 bytes of output) per iteration
            while i + 16 <= count {
                let chunk0 = _mm_loadl_epi64(src_ptr.add(i) as *const __m128i);
                let chunk1 = _mm_loadl_epi64(src_ptr.add(i + 8) as *const __m128i);

                let v0 = _mm256_cvtepu8_epi32(chunk0);
                let v1 = _mm256_cvtepu8_epi32(chunk1);

                _mm256_storeu_si256(dst_ptr.add(i) as *mut __m256i, v0);
                _mm256_storeu_si256(dst_ptr.add(i + 8) as *mut __m256i, v1);
                i += 16;
            }

            while i + 8 <= count {
                let chunk = _mm_loadl_epi64(src_ptr.add(i) as *const __m128i);
                let v = _mm256_cvtepu8_epi32(chunk);
                _mm256_storeu_si256(dst_ptr.add(i) as *mut __m256i, v);
                i += 8;
            }

            while i < count {
                *dst_ptr.add(i) = *src_ptr.add(i) as u32;
                i += 1;
            }
        }
    }

    /// AVX2-accelerated zero-extension unpacking of 16-bit integers to `u32`.
    ///
    /// # Safety
    /// Caller must ensure AVX2 target feature is supported on the host CPU.
    #[target_feature(enable = "avx2")]
    pub unsafe fn unpack_16bit_avx2(src: &[u8], dst: &mut [u32]) {
        let count = dst.len();
        let src_ptr = src.as_ptr() as *const u16;
        let dst_ptr = dst.as_mut_ptr();
        let mut i = 0;

        // SAFETY: Pointer arithmetic and vector loads/stores bounded by count.
        unsafe {
            // Process 16 elements (64 bytes of output) per iteration
            while i + 16 <= count {
                let chunk0 = _mm_loadu_si128(src_ptr.add(i) as *const __m128i);
                let chunk1 = _mm_loadu_si128(src_ptr.add(i + 8) as *const __m128i);

                let v0 = _mm256_cvtepu16_epi32(chunk0);
                let v1 = _mm256_cvtepu16_epi32(chunk1);

                _mm256_storeu_si256(dst_ptr.add(i) as *mut __m256i, v0);
                _mm256_storeu_si256(dst_ptr.add(i + 8) as *mut __m256i, v1);
                i += 16;
            }

            while i + 8 <= count {
                let chunk = _mm_loadu_si128(src_ptr.add(i) as *const __m128i);
                let v = _mm256_cvtepu16_epi32(chunk);
                _mm256_storeu_si256(dst_ptr.add(i) as *mut __m256i, v);
                i += 8;
            }

            while i < count {
                let b0 = *src.as_ptr().add(i * 2) as u32;
                let b1 = *src.as_ptr().add(i * 2 + 1) as u32;
                *dst_ptr.add(i) = b0 | (b1 << 8);
                i += 1;
            }
        }
    }
}
