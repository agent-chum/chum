//! `chum-daemon` — background daemon. Process supervisor and state owner.
//!
//! Per the architecture invariants in `CLAUDE.md`:
//! - All `start` / `stop` / `restart` flows go through this crate.
//! - It is registered with **launchd** at install time; launchd handles
//!   auto-start on user login, crash restart with backoff, and clean
//!   shutdown on logout.
//! - It exposes a protocol surface that `chum-cli` is a thin client for.
//!
//! This is the v0.0.1 scaffold — only `placeholder()` exists. The
//! supervisor, the launchd plist generation, and the protocol surface
//! land in subsequent commits.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder export so the crate compiles before real types land.
///
/// Will be removed in the first feature commit that introduces an actual
/// supervisor type.
pub fn placeholder() {}
