//! `chum-core` — shared types, manifest parsing, and signing primitives.
//!
//! Per the CHUM architecture invariants (see `CLAUDE.md`), this crate
//! performs **no I/O**. It holds pure types, schemas, and serialization
//! logic only. Anything that touches the filesystem, network, processes,
//! or launchd belongs in a different crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder export so the crate compiles before real types land.
///
/// Will be removed in the first feature commit that introduces an actual
/// manifest type.
pub fn placeholder() {}
