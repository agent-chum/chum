//! IPC protocol types + (de)serialization + stable wire codes.
//!
//! The wire format is **JSON Lines over a Unix domain socket**. Each
//! request is one line terminated by `\n`; the server replies with one
//! line and closes. v0.1 supports exactly one request per connection
//! — pipelining lands in a later session.
//!
//! ## Request
//!
//! ```json
//! {"protocol_version":1,"verb":"ping","args":null}
//! ```
//!
//! ## Response — ok
//!
//! ```json
//! {"protocol_version":1,"status":"ok","data":{...}}
//! ```
//!
//! ## Response — error
//!
//! ```json
//! {"protocol_version":1,"status":"error","code":"unknown_verb","message":"..."}
//! ```
//!
//! Stable machine codes live in [`codes`]. Bumping the protocol
//! version is a breaking change.

use serde::{Deserialize, Serialize};

pub mod client;
pub mod server;

pub use client::DaemonClient;

/// Current wire-protocol version. Bumped only on breaking changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// String returned in `ping.data.daemon_version` — bumped on any
/// daemon-visible behavior change, decoupled from `PROTOCOL_VERSION`.
pub const DAEMON_VERSION: &str = "0.1.0";

/// Stable machine codes for error envelopes. Scripts pattern-match on
/// these.
pub mod codes {
    /// The request's `protocol_version` is not understood by this
    /// daemon build. Caller should retry with a supported version
    /// (today: `1`).
    pub const UNSUPPORTED_PROTOCOL_VERSION: &str = "unsupported_protocol_version";
    /// `verb` is not a recognised string.
    pub const UNKNOWN_VERB: &str = "unknown_verb";
    /// Request body was empty or could not be parsed as JSON.
    pub const INVALID_REQUEST: &str = "invalid_request";
    /// Request line exceeded the daemon's hard size limit.
    pub const REQUEST_TOO_LARGE: &str = "request_too_large";
    /// Client did not finish sending the request within the read
    /// timeout.
    pub const REQUEST_TIMEOUT: &str = "request_timeout";
    /// Unrecoverable server-side fault — bug in the daemon.
    pub const INTERNAL: &str = "internal";

    // ----- Lifecycle verbs (spawn / terminate / restart / process_status) -----
    /// The registry has no row matching `(name, version)`.
    pub const PROCESS_NOT_INSTALLED: &str = "process_not_installed";
    /// `spawn` for a key whose Supervisor slot is in a non-terminal
    /// status.
    pub const PROCESS_ALREADY_RUNNING: &str = "process_already_running";
    /// `terminate` / `restart` for a key with no Supervisor slot.
    pub const PROCESS_NOT_RUNNING: &str = "process_not_running";
    /// `<install_dir>/chum-manifest.toml` is missing — install
    /// happened before this commit landed, or the directory was
    /// hand-edited.
    pub const MANIFEST_MISSING_IN_INSTALL_DIR: &str = "manifest_missing_in_install_dir";
    /// The on-disk manifest exists but `chum_core::parse_and_validate`
    /// rejected it.
    pub const MANIFEST_INVALID: &str = "manifest_invalid";
    /// `tokio::process::Command::spawn` failed inside `Supervisor::spawn`.
    pub const SPAWN_FAILED: &str = "spawn_failed";
    /// Signal delivery failed during `terminate` / `restart`.
    pub const KILL_FAILED: &str = "kill_failed";
    /// The supervisor's monitor task ended without writing a terminal
    /// status. Should not happen in normal operation.
    pub const MONITOR_WEDGED: &str = "monitor_wedged";

    // ----- tail_logs -----
    /// `tail_logs` was asked for a stream other than
    /// `"stdout" | "stderr" | "both"`.
    pub const LOGS_INVALID_STREAM: &str = "logs_invalid_stream";
    /// `tail_logs` was asked for more than the daemon's per-call
    /// line cap (10_000).
    pub const LOGS_LINES_TOO_LARGE: &str = "logs_lines_too_large";
    /// The package is installed but has no log files yet — start it
    /// (`chum start <name>`) and re-try.
    pub const LOGS_UNAVAILABLE: &str = "logs_unavailable";

    // ----- Broker -----
    /// `spawn` refused because the manifest declares permissions the
    /// user has not granted. The cli renders the unmet requirements
    /// as one `chum permit --grant <kind>=<value>` line per item.
    pub const PERMISSION_DENIED: &str = "permission_denied";
}

/// A request from a client to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Wire-protocol version. Must equal [`PROTOCOL_VERSION`] for
    /// this build, else the daemon returns
    /// [`codes::UNSUPPORTED_PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// Verb name, e.g. `"ping"`. See [`server`] for the dispatch
    /// table.
    pub verb: String,
    /// Optional verb-specific arguments. Defaults to JSON null.
    #[serde(default)]
    pub args: serde_json::Value,
}

/// A response from the daemon to a client.
///
/// Serializes with an internal tag on the `status` field, matching
/// the wire format documented at the module top.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Response {
    /// Successful response carrying verb-specific `data`.
    Ok {
        /// Wire-protocol version of this response.
        protocol_version: u32,
        /// Verb-specific payload — see typed response structs in
        /// this module for v0.1 shapes.
        data: serde_json::Value,
    },
    /// Error response.
    Error {
        /// Wire-protocol version of this response.
        protocol_version: u32,
        /// Stable machine code from [`codes`].
        code: String,
        /// Human-readable message. Do not pattern-match on this.
        message: String,
    },
}

impl Response {
    /// Build an `Ok` response with the current `PROTOCOL_VERSION`.
    pub fn ok(data: serde_json::Value) -> Self {
        Self::Ok {
            protocol_version: PROTOCOL_VERSION,
            data,
        }
    }

    /// Build an `Error` response with the current `PROTOCOL_VERSION`.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            protocol_version: PROTOCOL_VERSION,
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Typed payload for `ping`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResponse {
    /// Daemon build version (mirrors [`DAEMON_VERSION`]).
    pub daemon_version: String,
    /// Whole seconds since the daemon started.
    pub uptime_secs: u64,
    /// Number of installed packages observed at daemon startup.
    pub installed_count: u32,
}

/// Typed payload for `status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Daemon's own OS pid.
    pub pid: u32,
    /// RFC3339 timestamp of when the daemon process started.
    pub started_at: String,
    /// Installed-package snapshot (frozen at daemon startup; see
    /// `installed_count` rustdoc on `DaemonState`).
    pub installed_count: u32,
    /// Number of supervisor-managed processes — always 0 in v0.1
    /// (no spawn calls).
    pub running_count: u32,
}

/// One entry inside [`ListProcessesResponse::processes`].
///
/// All fields except `name` / `version` / `status` / `restart_count`
/// are optional — `pid` is missing when the process is between
/// waits, `exit_code` is only present on a `failed` status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedProcess {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// String form of the supervisor's `ProcessStatus`:
    /// `"starting" | "running" | "restarting" | "stopped" | "failed"`.
    pub status: String,
    /// Current OS pid; absent when the process has terminated or is
    /// between waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// User-driven restart count (incremented by the `restart` verb).
    pub restart_count: u32,
    /// Set on `status == "failed"` — the OS exit code reported by
    /// the kernel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Typed payload for `list_processes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProcessesResponse {
    /// Currently-supervised processes. Empty when no `spawn` has
    /// landed since daemon startup.
    pub processes: Vec<ListedProcess>,
}

/// Typed payload for `spawn`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResponse {
    /// Pid the supervisor assigned to the freshly spawned child.
    pub pid: u32,
    /// RFC3339 timestamp the child was spawned at.
    pub started_at: String,
}

/// Typed payload for `terminate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminateResponse {
    /// Always `true` for an ok response (the verb is action-only —
    /// returning a discriminator field keeps the JSON envelope
    /// non-empty, which matches the rest of the protocol).
    pub stopped: bool,
}

/// Typed payload for `restart`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartProcessResponse {
    /// Pid of the new child (different from the pre-restart pid).
    pub pid: u32,
    /// RFC3339 timestamp the new child was spawned at.
    pub started_at: String,
    /// Number of times the user has invoked `restart` for this key
    /// since the most recent `spawn`. Daemon-tracked — distinct from
    /// the supervisor's internal restart_count which counts
    /// policy-driven respawns.
    pub restart_count: u32,
}

/// Typed payload for `process_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStatusResponse {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// String form of the supervisor's `ProcessStatus`.
    pub status: String,
    /// Current OS pid, when the process is alive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// User-driven restart count.
    pub restart_count: u32,
    /// Exit code on `status == "failed"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Args envelope shared by `spawn`, `restart`, `process_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessKeyArgs {
    /// Package name from the registry / manifest.
    pub name: String,
    /// Package version from the registry / manifest.
    pub version: String,
}

/// Args envelope for `terminate`. Adds an optional grace duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminateArgs {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Seconds to wait between SIGTERM and SIGKILL. Defaults to 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_secs: Option<u64>,
}

/// Args envelope for `tail_logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailLogsArgs {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Which stream to return: `"stdout"`, `"stderr"`, or `"both"`.
    /// Defaults to `"both"`.
    #[serde(default = "default_tail_stream")]
    pub stream: String,
    /// Last N lines to return. Defaults to 100. Capped at
    /// [`super::server::MAX_TAIL_LINES`].
    #[serde(default = "default_tail_lines")]
    pub lines: usize,
}

fn default_tail_stream() -> String {
    "both".to_string()
}

fn default_tail_lines() -> usize {
    100
}

/// Typed payload for `tail_logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailLogsResponse {
    /// Echo of the requested stream (`"stdout"`, `"stderr"`, or
    /// `"both"`).
    pub stream: String,
    /// Joined-with-`\n` body. For `stream="both"`, the body is
    /// `=== stdout.log ===\n<stdout>\n=== stderr.log ===\n<stderr>`
    /// (each section bounded to the requested line count).
    pub content: String,
}
