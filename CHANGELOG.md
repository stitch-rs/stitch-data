<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rebooted Compact Binary Protocol (CBP) into a clean, modern Rust workspace.
- `crates/cbp-core`: `#![no_std]` framing engine implementing Version 1 Axiom,
  branchless `PrefixVarint`, `ZigZag`, Castagnoli `CRC32`, and `BitWriter` /
  `BitReader`.
- `crates/cbp-transforms`: Monomorphic transform pipelines (Delta-encoding,
  Bit-packing, RLE) with `stitch-core` integration.
- `crates/cbp`: High-performance facade crate.
- `benches`: Criterion benchmark harness for cross-ecosystem comparisons.
- `docs/RFC_WIRE_FORMAT_V1.md`: Formal wire format specification v1.0.

## [0.1.0] - 2025-10-04

### Added

- Initial project structure (legacy prototype).
