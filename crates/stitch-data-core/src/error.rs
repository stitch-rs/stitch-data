//! Typed error hierarchy for the Stitch Data Engine.

use core::fmt;

/// Primary error enum for all Stitch Data core operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchError {
    /// Provided slice or stream ended prematurely.
    UnexpectedEof {
        /// Bytes required for the operation.
        required: usize,
        /// Bytes available in buffer.
        available: usize,
    },
    /// The stream does not begin with the valid Stitch magic byte (`0xCB`).
    InvalidMagic(u8),
    /// The stream protocol version is not supported by this engine.
    UnsupportedVersion(u8),
    /// Invalid bitfield encoding in the frame header.
    InvalidHeaderFlags(u8),
    /// PrefixVarint format is malformed or exceeds maximum integer range.
    MalformedVarint,
    /// Integrity check failure: calculated CRC32 does not match expected footer CRC32.
    ChecksumMismatch {
        /// CRC32 found in the frame footer.
        expected: u32,
        /// CRC32 calculated over header and payload.
        actual: u32,
    },
    /// Specified bit width is out of range for the data type.
    InvalidBitWidth(u8),
    /// Schema hash mismatch: frame header schema hash does not match expected struct schema fingerprint.
    SchemaMismatch {
        /// Expected schema hash from compiled struct definition.
        expected: u64,
        /// Actual schema hash present in the frame header.
        actual: u64,
    },
    /// General payload decoding or unpacking failure.
    PayloadDecodeError,
    /// Frame payload length exceeds maximum permitted limit.
    FrameTooLarge {
        /// Specified payload length in bytes.
        size: u64,
        /// Maximum permitted payload length in bytes.
        limit: usize,
    },
}

/// Backward compatibility alias.
pub type CbpError = StitchError;

impl fmt::Display for StitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof {
                required,
                available,
            } => {
                write!(
                    f,
                    "unexpected EOF: required {required} bytes, available {available}"
                )
            }
            Self::InvalidMagic(m) => write!(f, "invalid Stitch magic: 0x{m:02X}, expected 0xCB"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported Stitch version: {v}"),
            Self::InvalidHeaderFlags(fl) => write!(f, "invalid header flags byte: 0x{fl:02X}"),
            Self::MalformedVarint => write!(f, "malformed prefix varint encoding"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: expected 0x{expected:08X}, actual 0x{actual:08X}"
                )
            }
            Self::InvalidBitWidth(w) => write!(f, "invalid bit width: {w}"),
            Self::SchemaMismatch { expected, actual } => {
                write!(
                    f,
                    "schema fingerprint mismatch: expected 0x{expected:016X}, actual 0x{actual:016X}"
                )
            }
            Self::PayloadDecodeError => write!(f, "payload decode error"),
            Self::FrameTooLarge { size, limit } => {
                write!(
                    f,
                    "frame payload size {size} bytes exceeds maximum limit of {limit} bytes"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StitchError {}
