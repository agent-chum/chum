//! v0.1 manifest schema for CHUM MCP server packages.
//!
//! A manifest describes one installable MCP server: identity, source,
//! runtime, lifecycle, health, declared capabilities, plus forward-compat
//! placeholders for permissions (v0.2) and signatures (v0.3).
//!
//! The shape is designed to extend through v0.5 without breaking older
//! manifests: new variants and fields land behind a `schema_version` bump,
//! and older parsers reject newer manifests cleanly.

mod parse;
pub mod permissions;
mod validate;

pub use parse::{parse_and_validate, parse_str};
pub use permissions::{
    EnvPermissions, FilesystemPermissions, NetworkPermissions, PermissionKind,
    PermissionRequirement, Permissions, SubprocessPermissions,
};
pub use validate::validate;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The manifest `schema_version` this build of chum-core supports.
///
/// Bumped at every breaking schema change. Older parsers refuse newer
/// manifests via [`crate::ManifestError::UnsupportedSchemaVersion`].
pub const SCHEMA_VERSION: &str = "0.1";

/// Top-level CHUM manifest.
///
/// Round-trips losslessly via [`toml::from_str`] + [`toml::to_string`] when
/// the input does not rely on optional defaults — defaults are normalised
/// on re-serialisation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version. Must equal [`SCHEMA_VERSION`] for this
    /// build of chum-core; older manifests are rejected at parse time.
    pub schema_version: String,
    /// Identity metadata for the packaged MCP server.
    pub package: Package,
    /// Where to fetch / install the server from.
    pub source: Source,
    /// How to start the server.
    pub runtime: Runtime,
    /// Restart, startup, and shutdown behaviour. Defaults applied when
    /// omitted; see [`Lifecycle::default`].
    #[serde(default)]
    pub lifecycle: Lifecycle,
    /// Health-check strategy. Defaults to process-alive only.
    #[serde(default)]
    pub health: Health,
    /// Declared capabilities. Informational in v0.1; enforced in v0.2.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Declared permissions (bookkeeping in v0.1, enforcement in v0.2).
    /// Empty by default — manifests without a `[permissions]` block
    /// require nothing and the broker auto-allows their spawn.
    #[serde(default)]
    pub permissions: Permissions,
    /// Forward-compat placeholder for v0.3 signature fields. Accepts any
    /// TOML content; v0.1 chum-core does not interpret it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<toml::Table>,
}

/// Identity metadata for the packaged MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Short package name. Must match `[a-z][a-z0-9-]{0,62}`.
    pub name: String,
    /// Package version. Semver-ish; non-empty.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// SPDX-ish license identifier.
    pub license: String,
    /// Author / maintainer list.
    pub authors: Vec<String>,
    /// Free-form tags for search and categorisation.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Where the MCP server is installed from.
///
/// The `Registry` variant lands in v0.5 behind a `schema_version` bump;
/// new variants extend this enum without breaking older manifests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// Install via `npm` / `npx`.
    Npm {
        /// npm package name (may include scope).
        package: String,
        /// Version range (semver-style).
        version: String,
    },
    /// Install via PyPI, typically through `uvx` or `pipx`.
    Pypi {
        /// PyPI package name.
        package: String,
        /// Version range.
        version: String,
    },
    /// Install from a git repository.
    Github {
        /// `owner/repo` slug.
        repo: String,
        /// Commit, tag, or branch.
        rev: String,
        /// Optional subdirectory of the repo to install from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdir: Option<String>,
    },
    /// Install from a local filesystem path. Intended for development
    /// manifests pointing at a working tree.
    Local {
        /// Absolute or manifest-relative path to the server source / binary.
        path: String,
    },
    /// Install from a downloadable binary release.
    Binary {
        /// HTTPS URL of the binary.
        url: String,
        /// 64-character lowercase hex SHA-256 of the downloaded artefact.
        checksum_sha256: String,
        /// Optional target triple (e.g. `aarch64-apple-darwin`) for
        /// multi-platform releases.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_triple: Option<String>,
    },
}

/// How to start the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// Executable to invoke.
    pub command: String,
    /// Arguments passed to the executable. Strings are literal — chum-core
    /// does not substitute `${VAR}` syntax; that is install-layer concern.
    #[serde(default)]
    pub args: Vec<String>,
    /// Transport surface the server exposes.
    pub transport: Transport,
    /// Environment variables. `BTreeMap` is deliberate — deterministic
    /// ordering matters for round-trip serialisation tests.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Transport surface for the MCP server.
///
/// Variants are extensible: new transports land behind a `schema_version`
/// bump. v0.1 supports stdio (most common), HTTP, and SSE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Transport {
    /// stdio MCP server. Spawned and supervised as a child of `chum-daemon`.
    Stdio,
    /// HTTP MCP server. Loopback-bound by default.
    Http {
        /// Port the server listens on.
        port: u16,
        /// Bind address. Defaults to `127.0.0.1`. `0.0.0.0` and `::` are
        /// rejected at validate time — CHUM is local-first.
        #[serde(default = "default_bind_address")]
        bind: String,
        /// Optional URL path component.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Server-Sent Events MCP server. Loopback-bound by default.
    Sse {
        /// Port the server listens on.
        port: u16,
        /// Bind address. Defaults to `127.0.0.1`. `0.0.0.0` and `::` are
        /// rejected at validate time — CHUM is local-first.
        #[serde(default = "default_bind_address")]
        bind: String,
        /// Optional URL path component.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

/// Restart, startup, and shutdown behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    /// When to restart the server after exit.
    #[serde(default)]
    pub restart: RestartPolicy,
    /// Seconds to wait for the server to come up before declaring failure.
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_sec: u32,
    /// Seconds to wait for graceful shutdown before SIGKILL.
    #[serde(default = "default_shutdown_grace")]
    pub shutdown_grace_sec: u32,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            restart: RestartPolicy::default(),
            startup_timeout_sec: default_startup_timeout(),
            shutdown_grace_sec: default_shutdown_grace(),
        }
    }
}

fn default_startup_timeout() -> u32 {
    10
}

fn default_shutdown_grace() -> u32 {
    5
}

/// Restart policy for a managed MCP server.
///
/// `kebab-case` wire format: `always` / `on-failure` / `never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    /// Restart the server unconditionally on exit.
    Always,
    /// Restart only on non-zero exit. The v0.1 default.
    #[default]
    OnFailure,
    /// Never restart; let the supervisor mark the server stopped.
    Never,
}

/// Health-check strategy.
///
/// Only `Process` is wired in v0.1 (process-alive). `Ping` and `Http` may
/// be declared and round-tripped but are not yet executed by `chum-daemon`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Health {
    /// Healthy as long as the child process is alive.
    Process,
    /// Liveness ping over the chosen transport.
    Ping {
        /// Seconds between consecutive pings.
        interval_sec: u32,
        /// Seconds before a ping is treated as failed.
        timeout_sec: u32,
    },
    /// HTTP health endpoint check.
    Http {
        /// Full URL to GET.
        url: String,
        /// Status code that signals healthy.
        expect_status: u16,
        /// Seconds between consecutive checks.
        interval_sec: u32,
        /// Seconds before a request is treated as failed.
        timeout_sec: u32,
    },
}

impl Default for Health {
    fn default() -> Self {
        Self::Process
    }
}

/// Declared capabilities of the MCP server.
///
/// Lists are informational in v0.1 — the broker enforces them in v0.2.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Names of MCP tools this server exposes.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Names of MCP resources this server exposes.
    #[serde(default)]
    pub resources: Vec<String>,
    /// Names of MCP prompts this server exposes.
    #[serde(default)]
    pub prompts: Vec<String>,
}
