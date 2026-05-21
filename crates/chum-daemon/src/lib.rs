//! `chum-daemon` — background daemon. Process supervisor, IPC server,
//! and state owner.
//!
//! v0.1 surface:
//! - [`Supervisor`] — in-memory process supervisor (spawn, monitor,
//!   restart, kill). See `supervisor/` module docs.
//! - [`DaemonClient`] — async client for the IPC socket; used by
//!   `chum-cli` and tests.
//! - [`ipc`] — wire-protocol types, server, codes.
//!
//! Architecture invariants (`CLAUDE.md`):
//! - All `start` / `stop` / `restart` flows go through this crate.
//! - It registers with **launchd** at install time (future work).
//! - It exposes a protocol surface that `chum-cli` is a thin client
//!   for. The IPC client lives here so the cli imports it directly.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod ipc;
pub mod supervisor;

pub use error::{IpcError, SupervisorError};
pub use ipc::{
    DAEMON_VERSION, DaemonClient, ListProcessesResponse, ListedProcess, PROTOCOL_VERSION,
    PingResponse, ProcessKeyArgs, ProcessStatusResponse, Request, Response,
    RestartProcessResponse, SpawnResponse, StatusResponse, TailLogsArgs, TailLogsResponse,
    TerminateArgs, TerminateResponse, codes, server::DaemonState,
};
pub use supervisor::restart::BackoffPolicy;
pub use supervisor::{ProcessHandle, ProcessKey, ProcessStatus, Supervisor};
