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

/// One entry inside [`ListProcessesResponse::processes`]. The shape
/// is locked at v0.1; entries are always empty in this session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedProcess {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// String form of the supervisor's `ProcessStatus`.
    pub status: String,
}

/// Typed payload for `list_processes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProcessesResponse {
    /// Currently-supervised processes. Always empty in v0.1.
    pub processes: Vec<ListedProcess>,
}
