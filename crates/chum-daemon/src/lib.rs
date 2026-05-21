//! `chum-daemon` — background daemon. Process supervisor and state owner.
//!
//! v0.1 scope shipped so far: the [`Supervisor`] primitive — an
//! in-memory supervisor that spawns, monitors, restarts, and kills
//! child processes described by an [`chum_install::InstalledArtifact`]
//! plus the originating [`chum_core::Manifest`].
//!
//! The daemon binary, IPC surface, launchd integration, and MCP
//! protocol awareness all land in subsequent sessions. Per the
//! architecture invariants in `CLAUDE.md`:
//! - All `start` / `stop` / `restart` flows go through this crate.
//! - It registers with **launchd** at install time (future work).
//! - It exposes a protocol surface that `chum-cli` is a thin client for
//!   (future work).
//!
//! The Supervisor is exposed as a library type today so other crates
//! can drive it directly while the daemon binary is being built out.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod supervisor;

pub use error::SupervisorError;
pub use supervisor::restart::BackoffPolicy;
pub use supervisor::{ProcessHandle, ProcessKey, ProcessStatus, Supervisor};
