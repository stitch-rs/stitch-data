//! `stitch-core` integration providing compile-time monomorphic transform pipelines.

use stitch_core::flow::FlowControl;
use stitch_core::middleware::{Layer, Terminal};
use stitch_data_core::error::StitchError;
use stitch_data_core::frame::OptimizationProfile;
use stitch_data_core::zigzag::{decode_zigzag32, encode_zigzag32};

use crate::bitpack::BitPacker;
use crate::delta::{delta_decode_u32, delta_encode_u32};
use crate::rle::RleTransformer;

/// Context state carried through the transformation pipeline.
#[repr(C, align(64))]
pub struct TransformContext<'a> {
    /// Temporary scratch buffer for intermediate transformation stages.
    pub scratch: &'a mut [u32],
    /// Destination byte buffer for packed bitstream output.
    pub out_buf: &'a mut [u8],
    /// Baseline value preserved from `input[0]`.
    pub baseline: u32,
    /// Final bytes written to `out_buf`.
    pub bytes_written: usize,
    /// L1D padding reserve to guarantee strictly 64-byte cache line alignment.
    pub _padding: [u8; 16],
}

impl<'a> stitch_core::Blackboard for TransformContext<'a> {}

const _: () = {
    assert!(core::mem::align_of::<TransformContext>() == 64);
    assert!(core::mem::size_of::<TransformContext>() == 64);
};

/// Stage 1: In-place monotone sequence Delta-encoding layer.
pub struct DeltaLayer;

impl<'a> Layer<TransformContext<'a>, (), usize, StitchError> for DeltaLayer {
    #[inline(always)]
    fn on_enter(
        &self,
        ctx: &mut TransformContext<'a>,
        _intent: (),
    ) -> FlowControl<(), usize, StitchError> {
        if !ctx.scratch.is_empty() {
            ctx.baseline = ctx.scratch[0];
            delta_encode_u32(ctx.scratch);
        }
        FlowControl::Proceed(())
    }

    #[inline(always)]
    fn on_exit(&self, _ctx: &mut TransformContext<'a>, _outcome: &mut Result<usize, StitchError>) {}
}

/// Stage 2: Terminal Bit-Packing execution handler.
/// Packs `[baseline: 4 bytes]` + `[BitPacked deltas]` into destination buffer.
pub struct BitPackTerminal;

impl<'a> Terminal<TransformContext<'a>, (), usize, StitchError> for BitPackTerminal {
    #[inline(always)]
    fn execute(
        &mut self,
        ctx: &mut TransformContext<'a>,
        _intent: (),
    ) -> Result<usize, StitchError> {
        if ctx.scratch.is_empty() {
            return Ok(0);
        }
        if ctx.out_buf.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: ctx.out_buf.len(),
            });
        }

        // Write 4-byte baseline
        ctx.out_buf[..4].copy_from_slice(&ctx.baseline.to_le_bytes());

        // Delta slice for remaining elements
        let deltas = &ctx.scratch[1..];
        let packed_len = BitPacker::pack_u32(deltas, &mut ctx.out_buf[4..])?;

        let total = 4 + packed_len;
        ctx.bytes_written = total;
        Ok(total)
    }
}

/// Monomorphic compression pipeline orchestrating Delta + BitPacking via SMA U-Cycle.
pub struct CompressionPipeline {
    delta: DeltaLayer,
    terminal: BitPackTerminal,
}

impl Default for CompressionPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl CompressionPipeline {
    /// Creates a new default compression pipeline.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            delta: DeltaLayer,
            terminal: BitPackTerminal,
        }
    }

    /// Compresses a slice of `u32` through the default (-O2 Balanced) pipeline.
    /// `scratch` must be at least as large as `input`.
    #[inline]
    pub fn compress_u32(
        &mut self,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        self.compress_with_profile_u32(OptimizationProfile::Balanced, input, scratch, out_buf)
    }

    /// Compresses a slice of `u32` using the specified optimization profile (-O0 .. -O3).
    pub fn compress_with_profile_u32(
        &mut self,
        profile: OptimizationProfile,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        match profile {
            OptimizationProfile::Direct => Self::compress_direct_u32(input, out_buf),
            OptimizationProfile::Realtime => Self::compress_realtime_u32(input, out_buf),
            OptimizationProfile::Balanced => self.compress_balanced_u32(input, scratch, out_buf),
            OptimizationProfile::MaxDensity => {
                self.compress_max_density_u32(input, scratch, out_buf)
            }
        }
    }

    /// Compresses a slice of `u32` with a 1-byte self-describing profile tag.
    pub fn compress_tagged_u32(
        &mut self,
        profile: OptimizationProfile,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        if out_buf.is_empty() {
            return Err(StitchError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }
        out_buf[0] = profile as u8;
        let len = self.compress_with_profile_u32(profile, input, scratch, &mut out_buf[1..])?;
        Ok(1 + len)
    }

    /// -O0 Direct: raw memory copy (48+ GiB/s, zero transformations).
    pub fn compress_direct_u32(input: &[u32], out_buf: &mut [u8]) -> Result<usize, StitchError> {
        let required = input.len() * 4;
        if out_buf.len() < required {
            return Err(StitchError::UnexpectedEof {
                required,
                available: out_buf.len(),
            });
        }
        for (i, &val) in input.iter().enumerate() {
            out_buf[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
        Ok(required)
    }

    /// -O1 Realtime: byte-aligned integer packing (1, 2, or 4 bytes per item).
    /// Format: `[Mode: 1B (1, 2, or 4)]` + `[Aligned Values: Mode * len]`
    pub fn compress_realtime_u32(input: &[u32], out_buf: &mut [u8]) -> Result<usize, StitchError> {
        if input.is_empty() {
            return Ok(0);
        }
        if out_buf.is_empty() {
            return Err(StitchError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }

        let max = input.iter().copied().max().unwrap_or(0);
        if max <= 0xFF {
            let required = 1 + input.len();
            if out_buf.len() < required {
                return Err(StitchError::UnexpectedEof {
                    required,
                    available: out_buf.len(),
                });
            }
            out_buf[0] = 1;
            for (i, &val) in input.iter().enumerate() {
                out_buf[1 + i] = val as u8;
            }
            Ok(required)
        } else if max <= 0xFFFF {
            let required = 1 + input.len() * 2;
            if out_buf.len() < required {
                return Err(StitchError::UnexpectedEof {
                    required,
                    available: out_buf.len(),
                });
            }
            out_buf[0] = 2;
            for (i, &val) in input.iter().enumerate() {
                out_buf[1 + i * 2..1 + (i + 1) * 2].copy_from_slice(&(val as u16).to_le_bytes());
            }
            Ok(required)
        } else {
            let required = 1 + input.len() * 4;
            if out_buf.len() < required {
                return Err(StitchError::UnexpectedEof {
                    required,
                    available: out_buf.len(),
                });
            }
            out_buf[0] = 4;
            for (i, &val) in input.iter().enumerate() {
                out_buf[1 + i * 4..1 + (i + 1) * 4].copy_from_slice(&val.to_le_bytes());
            }
            Ok(required)
        }
    }

    /// -O2 Balanced: Monomorphic Delta + BitPacking pipeline.
    fn compress_balanced_u32(
        &mut self,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        if input.is_empty() {
            return Ok(0);
        }
        if scratch.len() < input.len() {
            return Err(StitchError::UnexpectedEof {
                required: input.len(),
                available: scratch.len(),
            });
        }
        scratch[..input.len()].copy_from_slice(input);

        let mut ctx = TransformContext {
            scratch: &mut scratch[..input.len()],
            out_buf,
            baseline: 0,
            bytes_written: 0,
            _padding: [0u8; 16],
        };

        // Execute SMA U-Cycle: on_enter (Delta) -> execute (BitPack) -> on_exit
        match self.delta.on_enter(&mut ctx, ()) {
            FlowControl::Proceed(()) => {
                let mut res = self.terminal.execute(&mut ctx, ());
                self.delta.on_exit(&mut ctx, &mut res);
                res
            }
            FlowControl::ShortCircuit(val) => {
                let mut res = Ok(val);
                self.delta.on_exit(&mut ctx, &mut res);
                res
            }
            FlowControl::Halt(err) => {
                let mut res = Err(err);
                self.delta.on_exit(&mut ctx, &mut res);
                res
            }
        }
    }

    /// -O3 MaxDensity: evaluates RLE vs Delta+BitPack and selects minimum byte footprint.
    /// Format: `[Strategy: 1B]` + `[Payload]`
    pub fn compress_max_density_u32(
        &mut self,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        if input.is_empty() {
            return Ok(0);
        }
        if out_buf.len() < 5 {
            return Err(StitchError::UnexpectedEof {
                required: 5,
                available: out_buf.len(),
            });
        }

        // Test Strategy 2: Pure RLE into out_buf[1..]
        let rle_res = RleTransformer::encode_u32(input, &mut out_buf[1..]);

        let is_highly_repetitive = if input.len() > 1 {
            let mut runs = 0usize;
            for i in 1..input.len() {
                if input[i] == input[i - 1] {
                    runs += 1;
                }
            }
            runs * 3 > input.len()
        } else {
            false
        };

        if let Some(rlen) = rle_res
            .ok()
            .filter(|&l| is_highly_repetitive && l < input.len() * 4)
        {
            out_buf[0] = 0x02;
            return Ok(1 + rlen);
        }

        // Test Strategy 1: Delta + BitPack into out_buf[1..]
        out_buf[0] = 0x01;
        let delta_len = self.compress_balanced_u32(input, scratch, &mut out_buf[1..])?;

        if let Some(rlen) = rle_res.ok().filter(|&l| l < delta_len) {
            out_buf[0] = 0x02;
            RleTransformer::encode_u32(input, &mut out_buf[1..])?;
            return Ok(1 + rlen);
        }

        Ok(1 + delta_len)
    }

    /// Compresses a slice of signed 32-bit integers via Delta + ZigZag + BitPack.
    pub fn compress_i32(
        input: &[i32],
        scratch_u32: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, StitchError> {
        if input.is_empty() {
            return Ok(0);
        }
        if scratch_u32.len() < input.len() {
            return Err(StitchError::UnexpectedEof {
                required: input.len(),
                available: scratch_u32.len(),
            });
        }
        if out_buf.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: out_buf.len(),
            });
        }

        let baseline = input[0];
        out_buf[..4].copy_from_slice(&baseline.to_le_bytes());

        if input.len() == 1 {
            return Ok(4);
        }

        let mut prev = baseline;
        for (i, &val) in input[1..].iter().enumerate() {
            let delta = val.wrapping_sub(prev);
            scratch_u32[i] = encode_zigzag32(delta);
            prev = val;
        }

        let packed_len = BitPacker::pack_u32(&scratch_u32[..input.len() - 1], &mut out_buf[4..])?;
        Ok(4 + packed_len)
    }
}

/// Monomorphic decompression pipeline restoring BitPacked and Delta-encoded data.
pub struct DecompressionPipeline;

impl DecompressionPipeline {
    /// Decompresses data packed with `CompressionPipeline` under default (-O2 Balanced) profile.
    #[inline]
    pub fn decompress_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        Self::decompress_with_profile_u32(OptimizationProfile::Balanced, src, dst)
    }

    /// Decompresses data packed under the specified optimization profile.
    pub fn decompress_with_profile_u32(
        profile: OptimizationProfile,
        src: &[u8],
        dst: &mut [u32],
    ) -> Result<usize, StitchError> {
        match profile {
            OptimizationProfile::Direct => Self::decompress_direct_u32(src, dst),
            OptimizationProfile::Realtime => Self::decompress_realtime_u32(src, dst),
            OptimizationProfile::Balanced => Self::decompress_balanced_u32(src, dst),
            OptimizationProfile::MaxDensity => Self::decompress_max_density_u32(src, dst),
        }
    }

    /// Decompresses data prefixed with a 1-byte profile tag.
    pub fn decompress_tagged_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        if src.is_empty() {
            return Err(StitchError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }
        let profile = OptimizationProfile::from_bits(src[0]);
        let len = Self::decompress_with_profile_u32(profile, &src[1..], dst)?;
        Ok(1 + len)
    }

    /// -O0 Direct decompression.
    pub fn decompress_direct_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        let required = dst.len() * 4;
        if src.len() < required {
            return Err(StitchError::UnexpectedEof {
                required,
                available: src.len(),
            });
        }
        for (i, item) in dst.iter_mut().enumerate() {
            *item = u32::from_le_bytes(src[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        Ok(required)
    }

    /// -O1 Realtime decompression.
    pub fn decompress_realtime_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.is_empty() {
            return Err(StitchError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }

        let mode = src[0];
        match mode {
            1 => {
                let required = 1 + dst.len();
                if src.len() < required {
                    return Err(StitchError::UnexpectedEof {
                        required,
                        available: src.len(),
                    });
                }
                for (i, item) in dst.iter_mut().enumerate() {
                    *item = src[1 + i] as u32;
                }
                Ok(required)
            }
            2 => {
                let required = 1 + dst.len() * 2;
                if src.len() < required {
                    return Err(StitchError::UnexpectedEof {
                        required,
                        available: src.len(),
                    });
                }
                for (i, item) in dst.iter_mut().enumerate() {
                    *item = u16::from_le_bytes(src[1 + i * 2..1 + (i + 1) * 2].try_into().unwrap())
                        as u32;
                }
                Ok(required)
            }
            4 => {
                let required = 1 + dst.len() * 4;
                if src.len() < required {
                    return Err(StitchError::UnexpectedEof {
                        required,
                        available: src.len(),
                    });
                }
                for (i, item) in dst.iter_mut().enumerate() {
                    *item = u32::from_le_bytes(src[1 + i * 4..1 + (i + 1) * 4].try_into().unwrap());
                }
                Ok(required)
            }
            _ => Err(StitchError::InvalidBitWidth(mode)),
        }
    }

    /// -O2 Balanced decompression.
    fn decompress_balanced_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: src.len(),
            });
        }

        let baseline = u32::from_le_bytes(src[..4].try_into().unwrap());
        dst[0] = baseline;

        if dst.len() > 1 {
            let consumed = BitPacker::unpack_u32(&src[4..], &mut dst[1..])?;
            delta_decode_u32(dst);
            Ok(4 + consumed)
        } else {
            Ok(4)
        }
    }

    /// -O3 MaxDensity decompression.
    pub fn decompress_max_density_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.is_empty() {
            return Err(StitchError::UnexpectedEof {
                required: 1,
                available: 0,
            });
        }

        let strategy = src[0];
        match strategy {
            0x01 => {
                let consumed = Self::decompress_balanced_u32(&src[1..], dst)?;
                Ok(1 + consumed)
            }
            0x02 => {
                let (_elements, bytes_consumed) = RleTransformer::decode_u32_ext(&src[1..], dst)?;
                Ok(1 + bytes_consumed)
            }
            _ => Err(StitchError::PayloadDecodeError),
        }
    }

    /// Decompresses signed 32-bit integers compressed with `compress_i32`.
    pub fn decompress_i32(
        src: &[u8],
        scratch_u32: &mut [u32],
        dst: &mut [i32],
    ) -> Result<usize, StitchError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.len() < 4 {
            return Err(StitchError::UnexpectedEof {
                required: 4,
                available: src.len(),
            });
        }

        let baseline = i32::from_le_bytes(src[..4].try_into().unwrap());
        dst[0] = baseline;

        if dst.len() == 1 {
            return Ok(4);
        }

        if scratch_u32.len() < dst.len() - 1 {
            return Err(StitchError::UnexpectedEof {
                required: dst.len() - 1,
                available: scratch_u32.len(),
            });
        }

        let consumed = BitPacker::unpack_u32(&src[4..], &mut scratch_u32[..dst.len() - 1])?;

        let mut prev = baseline;
        for (i, &zz) in scratch_u32[..dst.len() - 1].iter().enumerate() {
            let delta = decode_zigzag32(zz);
            let restored = prev.wrapping_add(delta);
            dst[1 + i] = restored;
            prev = restored;
        }

        Ok(4 + consumed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_compression_roundtrip() {
        let input = [1000u32, 1002, 1005, 1007, 1010, 1012, 1015, 1020];
        let mut scratch = [0u32; 8];
        let mut out_buf = [0u8; 64];

        let mut pipeline = CompressionPipeline::new();
        let bytes_written = pipeline
            .compress_u32(&input, &mut scratch, &mut out_buf)
            .unwrap();

        assert_eq!(bytes_written, 8);

        let mut restored = [0u32; 8];
        DecompressionPipeline::decompress_u32(&out_buf[..bytes_written], &mut restored).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn test_profile_direct_roundtrip() {
        let input = [10u32, 20, 30, 40, 50];
        let mut out_buf = [0u8; 32];
        let written = CompressionPipeline::compress_direct_u32(&input, &mut out_buf).unwrap();
        assert_eq!(written, 20);

        let mut restored = [0u32; 5];
        let read = DecompressionPipeline::decompress_direct_u32(&out_buf[..written], &mut restored)
            .unwrap();
        assert_eq!(read, 20);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_profile_realtime_roundtrip_modes() {
        // Mode 1: <= 0xFF
        let input_u8 = [5u32, 10, 255, 0, 12];
        let mut out_buf = [0u8; 32];
        let written = CompressionPipeline::compress_realtime_u32(&input_u8, &mut out_buf).unwrap();
        assert_eq!(written, 1 + 5);

        let mut restored = [0u32; 5];
        let read =
            DecompressionPipeline::decompress_realtime_u32(&out_buf[..written], &mut restored)
                .unwrap();
        assert_eq!(read, written);
        assert_eq!(restored, input_u8);

        // Mode 2: <= 0xFFFF
        let input_u16 = [500u32, 1000, 65535, 42];
        let written2 =
            CompressionPipeline::compress_realtime_u32(&input_u16, &mut out_buf).unwrap();
        assert_eq!(written2, 1 + 8);

        let mut restored2 = [0u32; 4];
        let read2 =
            DecompressionPipeline::decompress_realtime_u32(&out_buf[..written2], &mut restored2)
                .unwrap();
        assert_eq!(read2, written2);
        assert_eq!(restored2, input_u16);
    }

    #[test]
    fn test_profile_max_density_roundtrip_rle_and_delta() {
        let mut pipeline = CompressionPipeline::new();
        let mut scratch = [0u32; 64];
        let mut out_buf = [0u8; 256];

        // Highly repetitive sequence -> should trigger RLE strategy
        let repetitive = [42u32; 32];
        let written_rle = pipeline
            .compress_with_profile_u32(
                OptimizationProfile::MaxDensity,
                &repetitive,
                &mut scratch,
                &mut out_buf,
            )
            .unwrap();

        assert_eq!(out_buf[0], 0x02); // Strategy 0x02 (RLE)
        let mut restored = [0u32; 32];
        DecompressionPipeline::decompress_with_profile_u32(
            OptimizationProfile::MaxDensity,
            &out_buf[..written_rle],
            &mut restored,
        )
        .unwrap();
        assert_eq!(restored, repetitive);

        // Monotonic sequence -> should trigger Delta+BitPack
        let monotonic = [100u32, 102, 104, 106, 108, 110, 112, 114];
        let written_delta = pipeline
            .compress_with_profile_u32(
                OptimizationProfile::MaxDensity,
                &monotonic,
                &mut scratch[..8],
                &mut out_buf,
            )
            .unwrap();

        assert_eq!(out_buf[0], 0x01); // Strategy 0x01 (Delta)
        let mut restored_mono = [0u32; 8];
        DecompressionPipeline::decompress_with_profile_u32(
            OptimizationProfile::MaxDensity,
            &out_buf[..written_delta],
            &mut restored_mono,
        )
        .unwrap();
        assert_eq!(restored_mono, monotonic);
    }

    #[test]
    fn test_signed_i32_pipeline_roundtrip() {
        let original = [100i32, 102, 101, 105, 103, 108, 107];
        let mut scratch = [0u32; 8];
        let mut out_buf = [0u8; 64];

        let written =
            CompressionPipeline::compress_i32(&original, &mut scratch, &mut out_buf).unwrap();

        let mut restored = [0i32; 7];
        let mut scratch_dec = [0u32; 8];
        let read = DecompressionPipeline::decompress_i32(
            &out_buf[..written],
            &mut scratch_dec,
            &mut restored,
        )
        .unwrap();

        assert_eq!(read, written);
        assert_eq!(restored, original);
    }
}
