//! `stitch-core` integration providing compile-time monomorphic transform pipelines.

use cbp_core::error::CbpError;
use stitch_core::flow::FlowControl;
use stitch_core::middleware::{Layer, Terminal};

use crate::bitpack::BitPacker;
use crate::delta::{delta_decode_u32, delta_encode_u32};

/// Context state carried through the transformation pipeline.
pub struct TransformContext<'a> {
    /// Temporary scratch buffer for intermediate transformation stages.
    pub scratch: &'a mut [u32],
    /// Baseline value preserved from `input[0]`.
    pub baseline: u32,
    /// Destination byte buffer for packed bitstream output.
    pub out_buf: &'a mut [u8],
    /// Final bytes written to `out_buf`.
    pub bytes_written: usize,
}

/// Stage 1: In-place monotone sequence Delta-encoding layer.
pub struct DeltaLayer;

impl<'a> Layer<TransformContext<'a>, (), usize, CbpError> for DeltaLayer {
    #[inline(always)]
    fn on_enter(
        &self,
        ctx: &mut TransformContext<'a>,
        _intent: (),
    ) -> FlowControl<(), usize, CbpError> {
        if !ctx.scratch.is_empty() {
            ctx.baseline = ctx.scratch[0];
            delta_encode_u32(ctx.scratch);
        }
        FlowControl::Proceed(())
    }

    #[inline(always)]
    fn on_exit(&self, _ctx: &mut TransformContext<'a>, _outcome: &mut Result<usize, CbpError>) {}
}

/// Stage 2: Terminal Bit-Packing execution handler.
/// Packs `[baseline: 4 bytes]` + `[BitPacked deltas]` into destination buffer.
pub struct BitPackTerminal;

impl<'a> Terminal<TransformContext<'a>, (), usize, CbpError> for BitPackTerminal {
    #[inline(always)]
    fn execute(&mut self, ctx: &mut TransformContext<'a>, _intent: ()) -> Result<usize, CbpError> {
        if ctx.scratch.is_empty() {
            return Ok(0);
        }
        if ctx.out_buf.len() < 4 {
            return Err(CbpError::UnexpectedEof {
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

    /// Compresses a slice of `u32` through the Delta + BitPacking pipeline.
    /// `scratch` must be at least as large as `input`.
    pub fn compress_u32(
        &mut self,
        input: &[u32],
        scratch: &mut [u32],
        out_buf: &mut [u8],
    ) -> Result<usize, CbpError> {
        if input.is_empty() {
            return Ok(0);
        }
        if scratch.len() < input.len() {
            return Err(CbpError::UnexpectedEof {
                required: input.len(),
                available: scratch.len(),
            });
        }
        scratch[..input.len()].copy_from_slice(input);

        let mut ctx = TransformContext {
            scratch: &mut scratch[..input.len()],
            baseline: 0,
            out_buf,
            bytes_written: 0,
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
}

/// Monomorphic decompression pipeline restoring BitPacked and Delta-encoded data.
pub struct DecompressionPipeline;

impl DecompressionPipeline {
    /// Decompresses data packed with `CompressionPipeline`.
    pub fn decompress_u32(src: &[u8], dst: &mut [u32]) -> Result<usize, CbpError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if src.len() < 4 {
            return Err(CbpError::UnexpectedEof {
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

        // 4 bytes baseline + 1 byte bit_width + ceil(7 items * 3 bits / 8) [3 bytes] = 8 bytes total (down from 32 bytes raw!)
        assert_eq!(bytes_written, 8);

        let mut restored = [0u32; 8];
        DecompressionPipeline::decompress_u32(&out_buf[..bytes_written], &mut restored).unwrap();

        assert_eq!(restored, input);
    }
}
