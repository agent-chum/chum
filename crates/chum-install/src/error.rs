//! Errors raised by chum-install operations.

use std::path::PathBuf;
use thiserror::Error;

/// All errors emitted by `chum-install`.
///
/// Variants are structured so callers (CLI, daemon, registry) can pattern-
/// match on outcome class without re-parsing error messages. The shape is
/// intentionally explicit — every field a downstream user might want to
/// log or surface is its own struct field.
#[derive(Debug, Error)]
pub enum InstallError {
    /// An external tool was not found on `PATH` (e.g. `npm`).
    #[error("missing tool `{tool}`: not found on PATH")]
    MissingTool {
        /// Name of the missing executable.
        tool: String,
    },

    /// HTTP fetch failed (timeout, connection refused, non-2xx status,
    /// TLS handshake failure, etc.).
    #[error("failed to fetch `{url}`: {source}")]
    FetchFailed {
        /// The URL that failed to fetch.
        url: String,
        /// The underlying error chain. Box<dyn> so the variant stays
        /// independent of reqwest's concrete error type.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The downloaded artefact's SHA-256 did not match the manifest's
    /// declared `checksum_sha256`. The download is **rejected**; no
    /// fallback or retry happens at this layer.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// The checksum the manifest declared (lowercase hex).
        expected: String,
        /// The checksum of the bytes actually received (lowercase hex).
        actual: String,
    },

    /// A subprocess (e.g. `npm install`) exited with non-zero status.
    #[error("subprocess `{cmd}` exited with status {exit}: {stderr}")]
    SubprocessFailed {
        /// Command that was invoked (program plus args, joined for display).
        cmd: String,
        /// Exit status code. `-1` indicates "killed by signal".
        exit: i32,
        /// Captured stderr from the subprocess.
        stderr: String,
    },

    /// Archive extraction failed — corrupt tar/zip, unsupported format,
    /// or an entry that escapes the extraction root.
    #[error("archive extraction failed: {0}")]
    ExtractFailed(String),

    /// The manifest declared a source kind that v0.1 `chum-install` does
    /// not implement. Forward-compat — `pypi`, `github`, and the v0.5
    /// `registry` kind will land in later versions.
    #[error("unsupported source kind `{0}` in v0.1 chum-install")]
    UnsupportedSource(String),

    /// A path traversal was detected and rejected — for example, a Local
    /// source that contains `..` components.
    #[error("path traversal rejected: `{0}`")]
    PathTraversal(PathBuf),

    /// Underlying I/O error from std / tokio.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
