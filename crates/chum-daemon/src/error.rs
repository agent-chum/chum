//! Errors raised by the in-memory process supervisor and the IPC
//! layer.

use std::path::PathBuf;

use thiserror::Error;

use crate::supervisor::ProcessKey;

/// All errors emitted by [`crate::supervisor::Supervisor`].
///
/// Variants are pattern-match-friendly so callers can distinguish a
/// duplicate spawn from a missing key without parsing message text.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// `spawn` was called for a key whose record already exists and
    /// is in a non-terminal status (`Starting | Running | Restarting`).
    /// Use `restart` to stop and re-spawn instead.
    #[error("'{key}' is already running")]
    AlreadyRunning {
        /// The conflicting key.
        key: ProcessKey,
    },

    /// `stop`, `kill`, `restart`, or `status` was called for a key
    /// that has no record in the supervisor.
    #[error("'{key}' is not running")]
    NotRunning {
        /// The missing key.
        key: ProcessKey,
    },

    /// Spawning the child process failed (typical causes: command
    /// not found, permission denied, working directory missing).
    #[error("spawn failed: {source}")]
    SpawnFailed {
        /// Underlying I/O error from `tokio::process::Command::spawn`.
        #[source]
        source: std::io::Error,
    },

    /// Signal delivery failed during a stop or kill request.
    /// Typically wraps an `nix` errno (e.g. `EPERM`) or the
    /// "child still alive after SIGKILL" hard-ceiling timeout.
    #[error("kill failed: {reason}")]
    KillFailed {
        /// Free-form reason. Stable across patch versions; do not
        /// pattern-match on the string.
        reason: String,
    },

    /// The monitor task ended without writing a terminal status —
    /// shouldn't happen in normal operation, but defense in depth
    /// keeps `stop`/`kill` from hanging forever if the monitor
    /// panics.
    #[error("supervisor monitor for '{key}' ended unexpectedly")]
    MonitorWedged {
        /// The key whose monitor exited without flipping to a
        /// terminal status.
        key: ProcessKey,
    },

    /// Underlying I/O error from `std` / `tokio`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Errors emitted by the IPC server and client.
///
/// Wire-format errors (unsupported protocol version, unknown verb,
/// request too large) are also exposed as stable string codes via the
/// [`crate::ipc::codes`] module so that scripts can pattern-match
/// against them without parsing free-form messages.
#[derive(Debug, Error)]
pub enum IpcError {
    /// `bind()` on the Unix socket failed.
    #[error("bind on {path}: {source}")]
    BindFailed {
        /// Socket path the daemon tried to bind to.
        path: PathBuf,
        /// Underlying I/O error from the OS.
        #[source]
        source: std::io::Error,
    },

    /// A live `chumd` already owns the target socket — startup must
    /// abort.
    #[error("another chumd appears to be running at {path}")]
    SocketAlreadyInUse {
        /// Existing socket path that responded to a connect probe.
        path: PathBuf,
    },

    /// `connect()` on the client side failed (no daemon running,
    /// socket file missing, permission denied).
    #[error("cannot reach daemon at {path}: {source}")]
    ConnectFailed {
        /// Socket path the client tried to reach.
        path: PathBuf,
        /// Underlying I/O error from the OS.
        #[source]
        source: std::io::Error,
    },

    /// A wire-protocol violation: malformed JSON, an unexpected shape,
    /// or a response that does not decode into the expected typed
    /// envelope.
    #[error("protocol error: {reason}")]
    ProtocolError {
        /// Free-form reason. Stable across patch versions; do not
        /// pattern-match on the string — pattern-match on the variant.
        reason: String,
    },

    /// The server returned an error envelope. Carries the stable
    /// `code` (see [`crate::ipc::codes`]) and the human message.
    #[error("daemon returned error: {code}: {message}")]
    ServerError {
        /// Stable machine code.
        code: String,
        /// Human-readable message from the server.
        message: String,
    },

    /// `serde_json` (de)serialisation failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Underlying I/O error from `std` / `tokio`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
