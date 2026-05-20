//! `chum-core` — shared types, manifest parsing, and signing primitives.
//!
//! Per the CHUM architecture invariants (see `CLAUDE.md`), this crate
//! performs **no I/O**. It holds pure types, schemas, and serialisation
//! logic only. Anything that touches the filesystem, network, processes,
//! or launchd belongs in a different crate.
//!
//! The v0.1 surface is the manifest schema:
//! [`manifest::parse_str`] reads a manifest TOML string,
//! [`manifest::validate`] performs semantic checks beyond serde,
//! and [`manifest::parse_and_validate`] composes the two.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod manifest;

pub use error::ManifestError;
pub use manifest::{Manifest, parse_str};
