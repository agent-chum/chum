//! User-facing error envelope and the renderer that maps every
//! crate-level error into a stable, machine-readable message.
//!
//! The CLI is the single point of error → user mapping for the v0.1
//! pipeline. Library crates raise typed errors (`ManifestError`,
//! `InstallError`, `RegistryError`); this module's [`UserFacingError`]
//! wraps them and [`render`] formats them for either humans (stderr)
//! or scripts (`--json` to stdout).
//!
//! ## Stable codes
//!
//! [`UserFacingError::code`] returns a `&'static str` that is part of
//! the machine-readable contract — scripts pattern-match on values
//! like `install_checksum_mismatch` or `already_installed`. Never
//! repurpose a code; introduce a new variant + new code if a new
//! error class lands.

use std::path::PathBuf;

use chum_core::ManifestError;
use chum_install::InstallError;
use chum_registry::RegistryError;

/// Every error class the install pipeline can surface to the user.
///
/// Variants stay distinct per error class even when their user-facing
/// messages overlap (e.g. `AlreadyInstalled` vs
/// `Registry(DuplicateArtifact)`) — so callers can match on cause,
/// not message text.
#[derive(Debug)]
pub enum UserFacingError {
    /// Could not read the manifest file off disk.
    ManifestIo {
        /// Path the caller asked us to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Manifest parse or validation failure from `chum-core`.
    Manifest(ManifestError),
    /// `chum-install` raised an error.
    Install(InstallError),
    /// `chum-registry` raised an error.
    Registry(RegistryError),
    /// Could not create or access the CHUM root directory.
    RootIo {
        /// Path of the root we tried to prepare.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Neither `--root`, `$CHUM_HOME`, `$XDG_DATA_HOME`, nor `$HOME`
    /// is set — there is nowhere to install.
    ChumHomeUnresolved,
    /// Pre-check found an existing row for `(name, version)`. The
    /// install refused to overwrite.
    AlreadyInstalled {
        /// Package name from the manifest.
        name: String,
        /// Package version from the manifest.
        version: String,
    },
    /// Uninstall asked for a name that has no rows in the registry.
    NotInstalled {
        /// Name the caller passed.
        name: String,
    },
    /// Uninstall was asked to remove `<name>` without a version, but
    /// more than one version is installed and the caller must pick.
    AmbiguousVersion {
        /// Name the caller passed.
        name: String,
        /// All versions currently installed for that name, in
        /// registry order.
        versions: Vec<String>,
    },
    /// `fs::remove_dir_all` (or equivalent) failed while tearing down
    /// a package's `install_dir`.
    RemoveFailed {
        /// Path the cli tried to remove.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Could not reach the chumd daemon over its IPC socket.
    DaemonUnreachable {
        /// Socket path the cli tried to connect to.
        path: PathBuf,
        /// Underlying I/O error from `connect`.
        source: std::io::Error,
    },
    /// The daemon responded but the response could not be
    /// interpreted: malformed JSON, unexpected shape, or an explicit
    /// error envelope. Carries a free-form reason; do not
    /// pattern-match on the string.
    DaemonProtocol {
        /// Free-form description of the failure.
        reason: String,
    },
    /// A lifecycle subcommand referenced a name that the registry
    /// doesn't know. `version` is `Some` when the caller supplied
    /// `--version` or a positional version explicitly; `None` when
    /// resolution failed before any version was settled.
    ProcessNotInstalled {
        /// Package name the caller passed.
        name: String,
        /// Version the caller asked for, if any.
        version: Option<String>,
    },
    /// `chum start` saw the daemon report `process_already_running`.
    ProcessAlreadyRunning {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
    },
    /// `chum stop` / `chum restart` saw the daemon report
    /// `process_not_running`.
    ProcessNotRunning {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
    },
    /// The daemon could not find `<install_dir>/chum-manifest.toml`.
    /// Typically means the row was installed before commit-1 of this
    /// session landed and the package needs to be re-installed.
    ManifestMissing {
        /// install_dir that should have contained the manifest.
        install_dir: PathBuf,
    },
    /// `chum logs` saw the daemon report `logs_unavailable` — the
    /// package is installed but has no log files yet. User must
    /// `chum start` it at least once to populate logs.
    LogsUnavailable {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
        /// install_dir whose `logs/` is empty.
        install_dir: PathBuf,
    },
    /// `chum daemon install-service` refused because a plist file
    /// already exists at the target path. Caller can re-run with
    /// `--force` to replace it.
    ServiceAlreadyInstalled {
        /// Path that's already taken.
        path: PathBuf,
    },
    /// A `launchctl` / filesystem command run by the service
    /// subcommands failed. Carries the command string for diagnostics
    /// and the captured stderr.
    ServiceCommandFailed {
        /// Command line we tried to run.
        cmd: String,
        /// Captured stderr (or a synthesized reason string).
        stderr: String,
    },
    /// `chum start` saw the daemon report `permission_denied` — the
    /// manifest declared permissions the user has not granted yet.
    PermissionDenied {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
        /// Declared-but-not-granted requirements in `kind=value` form.
        unmet: Vec<String>,
    },
    /// `--grant <input>` did not parse as a known
    /// `<kind>=<value>` pair.
    UnknownPermission {
        /// Literal string the user passed.
        input: String,
    },
    /// `chum revoke` was asked to remove a grant that doesn't exist.
    GrantNotFound {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
        /// Permission kind.
        kind: String,
        /// Permission value.
        value: String,
    },
    /// `chum env set` / `unset` saw a key that doesn't match
    /// `[A-Za-z_][A-Za-z0-9_]*`.
    EnvKeyInvalid {
        /// Literal key string the user passed.
        key: String,
    },
    /// Writing the updated manifest TOML back to disk failed.
    EnvUpdateFailed {
        /// Path we tried to write.
        path: PathBuf,
        /// Free-form reason (serialisation or I/O).
        reason: String,
    },
}

impl UserFacingError {
    /// Stable machine-readable code for this error. Part of the
    /// `--json` contract; scripts pattern-match on these.
    pub fn code(&self) -> &'static str {
        match self {
            UserFacingError::ManifestIo { .. } => "manifest_io",
            UserFacingError::Manifest(ManifestError::Toml(_)) => "manifest_invalid_toml",
            UserFacingError::Manifest(ManifestError::TomlSerialize(_)) => "manifest_serialize",
            UserFacingError::Manifest(ManifestError::MissingSchemaVersion) => {
                "manifest_missing_schema_version"
            }
            UserFacingError::Manifest(ManifestError::UnsupportedSchemaVersion(_)) => {
                "manifest_unsupported_schema_version"
            }
            UserFacingError::Manifest(ManifestError::InvalidName(_)) => "manifest_invalid_name",
            UserFacingError::Manifest(ManifestError::InvalidVersion(_)) => {
                "manifest_invalid_version"
            }
            UserFacingError::Manifest(ManifestError::InvalidChecksum(_)) => {
                "manifest_invalid_checksum"
            }
            UserFacingError::Manifest(ManifestError::InvalidUrl(_)) => "manifest_invalid_url",
            UserFacingError::Manifest(ManifestError::InvalidBindAddress(_)) => {
                "manifest_invalid_bind_address"
            }
            UserFacingError::Install(InstallError::MissingTool { .. }) => "install_missing_tool",
            UserFacingError::Install(InstallError::FetchFailed { .. }) => "install_fetch_failed",
            UserFacingError::Install(InstallError::ChecksumMismatch { .. }) => {
                "install_checksum_mismatch"
            }
            UserFacingError::Install(InstallError::SubprocessFailed { .. }) => {
                "install_subprocess_failed"
            }
            UserFacingError::Install(InstallError::ExtractFailed(_)) => "install_extract_failed",
            UserFacingError::Install(InstallError::UnsupportedSource(_)) => {
                "install_unsupported_source"
            }
            UserFacingError::Install(InstallError::PathTraversal(_)) => "install_path_traversal",
            UserFacingError::Install(InstallError::Io(_)) => "install_io",
            UserFacingError::Install(InstallError::ManifestSerialize(_)) => {
                "install_manifest_serialize"
            }
            UserFacingError::Registry(RegistryError::SqlError(_)) => "registry_sql",
            UserFacingError::Registry(RegistryError::MigrationFailed { .. }) => {
                "registry_migration"
            }
            UserFacingError::Registry(RegistryError::NotFound { .. }) => "registry_not_found",
            UserFacingError::Registry(RegistryError::DuplicateArtifact { .. }) => {
                "registry_duplicate"
            }
            UserFacingError::Registry(RegistryError::Io(_)) => "registry_io",
            UserFacingError::RootIo { .. } => "root_io",
            UserFacingError::ChumHomeUnresolved => "chum_home_unresolved",
            UserFacingError::AlreadyInstalled { .. } => "already_installed",
            UserFacingError::NotInstalled { .. } => "not_installed",
            UserFacingError::AmbiguousVersion { .. } => "ambiguous_version",
            UserFacingError::RemoveFailed { .. } => "remove_failed",
            UserFacingError::DaemonUnreachable { .. } => "daemon_unreachable",
            UserFacingError::DaemonProtocol { .. } => "daemon_protocol_error",
            UserFacingError::ProcessNotInstalled { .. } => "process_not_installed",
            UserFacingError::ProcessAlreadyRunning { .. } => "process_already_running",
            UserFacingError::ProcessNotRunning { .. } => "process_not_running",
            UserFacingError::ManifestMissing { .. } => "manifest_missing_in_install_dir",
            UserFacingError::LogsUnavailable { .. } => "logs_unavailable",
            UserFacingError::ServiceAlreadyInstalled { .. } => "service_already_installed",
            UserFacingError::ServiceCommandFailed { .. } => "service_command_failed",
            UserFacingError::PermissionDenied { .. } => "permission_denied",
            UserFacingError::UnknownPermission { .. } => "unknown_permission",
            UserFacingError::GrantNotFound { .. } => "grant_not_found",
            UserFacingError::EnvKeyInvalid { .. } => "env_key_invalid",
            UserFacingError::EnvUpdateFailed { .. } => "env_update_failed",
        }
    }

    /// Recovery hint for this error, or `None` when the existing
    /// [`Self::message`] is already actionable enough.
    ///
    /// Rendered as a `hint: <text>` line below the error message in
    /// human mode. Carried as a `"hint"` field in the `--json`
    /// envelope (omitted when `None`).
    pub fn recovery_hint(&self) -> Option<&'static str> {
        match self {
            UserFacingError::ManifestIo { .. } => {
                Some("check the path; you can list available manifests with: chum search")
            }
            UserFacingError::Manifest(ManifestError::Toml(_)) => {
                Some("fix the TOML syntax error and re-run chum install")
            }
            UserFacingError::Manifest(ManifestError::TomlSerialize(_)) => {
                Some("likely a chum bug — please file an issue")
            }
            UserFacingError::Install(InstallError::FetchFailed { .. }) => {
                Some("check the URL or your network connection")
            }
            UserFacingError::Install(InstallError::ChecksumMismatch { .. }) => {
                Some("the binary may have been tampered with — verify the manifest's checksum_sha256")
            }
            UserFacingError::Install(InstallError::SubprocessFailed { .. }) => {
                Some("check the subprocess's stderr above for the underlying error")
            }
            UserFacingError::Install(InstallError::ExtractFailed(_)) => {
                Some("the archive may be corrupted — re-download or verify the URL")
            }
            UserFacingError::Install(InstallError::UnsupportedSource(_)) => {
                Some("only npm / local / binary are supported in v0.1; pypi / github land in v0.2+")
            }
            UserFacingError::Install(InstallError::Io(_)) => {
                Some("check filesystem permissions on the install root")
            }
            UserFacingError::Install(InstallError::ManifestSerialize(_)) => {
                Some("likely a chum bug — please file an issue")
            }
            UserFacingError::Registry(RegistryError::SqlError(_)) => {
                Some("check that state.db is readable; try: chum doctor")
            }
            UserFacingError::Registry(RegistryError::MigrationFailed { .. }) => {
                Some("your state.db may be from a future chum version — back it up and retry")
            }
            UserFacingError::Registry(RegistryError::NotFound { .. }) => {
                Some("likely a chum bug — please file an issue")
            }
            UserFacingError::Registry(RegistryError::Io(_)) => {
                Some("check filesystem permissions on <chum_home>/state.db")
            }
            UserFacingError::RootIo { .. } => {
                Some("check filesystem permissions on the root path")
            }
            UserFacingError::RemoveFailed { .. } => {
                Some("check filesystem permissions on the package directory")
            }
            UserFacingError::NotInstalled { .. } => {
                Some("install with: chum install <manifest>")
            }
            UserFacingError::DaemonUnreachable { .. } => {
                Some("start chumd with: chumd & (or: chum daemon install-service for auto-start)")
            }
            UserFacingError::DaemonProtocol { .. } => {
                Some("likely a chum bug — please file an issue with the daemon's stderr")
            }
            UserFacingError::ProcessNotInstalled { .. } => {
                Some("install with: chum install <manifest>")
            }
            UserFacingError::ProcessNotRunning { .. } => {
                Some("start with: chum start <name>")
            }
            UserFacingError::ServiceCommandFailed { .. } => {
                Some("check the launchctl output above; common cause: chumd binary moved")
            }
            UserFacingError::GrantNotFound { .. } => {
                Some("run 'chum permissions <name>' to see all grants")
            }
            UserFacingError::EnvKeyInvalid { .. } => {
                Some("keys must match [A-Za-z_][A-Za-z0-9_]* (POSIX shape)")
            }
            UserFacingError::EnvUpdateFailed { .. } => {
                Some("check filesystem permissions on the manifest file")
            }
            // Variants whose `message()` already embeds the recovery
            // action don't need a second line.
            _ => None,
        }
    }

    /// Human-readable message for this error.
    pub fn message(&self) -> String {
        match self {
            UserFacingError::ManifestIo { path, source } => {
                format!("cannot read manifest at {}: {source}", path.display())
            }
            UserFacingError::Manifest(ManifestError::Toml(e)) => {
                format!("manifest has invalid TOML: {e}")
            }
            UserFacingError::Manifest(ManifestError::TomlSerialize(e)) => {
                format!("failed to round-trip manifest TOML: {e}")
            }
            UserFacingError::Manifest(ManifestError::MissingSchemaVersion) => {
                "manifest is missing required field 'schema_version'".to_string()
            }
            UserFacingError::Manifest(ManifestError::UnsupportedSchemaVersion(v)) => {
                format!(
                    "manifest schema version '{v}' is not supported (this chum supports '0.1')"
                )
            }
            UserFacingError::Manifest(ManifestError::InvalidName(n)) => {
                format!("invalid package name '{n}': must match [a-z][a-z0-9-]{{0,62}}")
            }
            UserFacingError::Manifest(ManifestError::InvalidVersion(v)) => {
                format!("invalid package version '{v}': must be non-empty")
            }
            UserFacingError::Manifest(ManifestError::InvalidChecksum(c)) => {
                format!("invalid sha256 checksum '{c}': expected 64 lowercase hex characters")
            }
            UserFacingError::Manifest(ManifestError::InvalidUrl(u)) => {
                format!("invalid url '{u}': must start with http:// or https://")
            }
            UserFacingError::Manifest(ManifestError::InvalidBindAddress(a)) => {
                format!("invalid bind address '{a}': CHUM is local-first; bind to a loopback address")
            }
            UserFacingError::Install(InstallError::MissingTool { tool }) => {
                format!("{tool} is required but not found in PATH; install it and retry")
            }
            UserFacingError::Install(InstallError::FetchFailed { url, source }) => {
                format!("could not fetch '{url}': {source}")
            }
            UserFacingError::Install(InstallError::ChecksumMismatch { expected, actual }) => {
                format!(
                    "binary checksum mismatch — refusing to install (expected {expected}, got {actual})"
                )
            }
            UserFacingError::Install(InstallError::SubprocessFailed { cmd, exit, stderr }) => {
                format!("'{cmd}' exited with status {exit}: {stderr}")
            }
            UserFacingError::Install(InstallError::ExtractFailed(msg)) => {
                format!("archive extraction failed: {msg}")
            }
            UserFacingError::Install(InstallError::UnsupportedSource(k)) => {
                format!("source kind '{k}' is not supported by this chum")
            }
            UserFacingError::Install(InstallError::PathTraversal(p)) => {
                format!(
                    "local source path '{}' rejected: must be absolute with no '..' components",
                    p.display()
                )
            }
            UserFacingError::Install(InstallError::Io(e)) => {
                format!("i/o error during install: {e}")
            }
            UserFacingError::Install(InstallError::ManifestSerialize(e)) => {
                format!("could not serialize manifest into install_dir: {e}")
            }
            UserFacingError::Registry(RegistryError::SqlError(e)) => {
                format!("registry sqlite error: {e}")
            }
            UserFacingError::Registry(RegistryError::MigrationFailed { reason }) => {
                format!("registry schema migration failed: {reason}")
            }
            UserFacingError::Registry(RegistryError::NotFound { name, version }) => {
                format!(
                    "registry unexpectedly missing '{name}' {version} mid-install (defense-in-depth path)"
                )
            }
            UserFacingError::Registry(RegistryError::DuplicateArtifact { name, version }) => {
                format!(
                    "'{name}' {version} is already installed (run 'chum uninstall {name}' first)"
                )
            }
            UserFacingError::Registry(RegistryError::Io(e)) => {
                format!("registry i/o error: {e}")
            }
            UserFacingError::RootIo { path, source } => {
                format!("cannot prepare CHUM root at {}: {source}", path.display())
            }
            UserFacingError::ChumHomeUnresolved => "cannot resolve CHUM root: set --root, $CHUM_HOME, $XDG_DATA_HOME, or $HOME".to_string(),
            UserFacingError::AlreadyInstalled { name, version } => {
                format!(
                    "'{name}' {version} is already installed (run 'chum uninstall {name}' first)"
                )
            }
            UserFacingError::NotInstalled { name } => {
                format!("'{name}' is not installed")
            }
            UserFacingError::AmbiguousVersion { name, versions } => {
                let list = versions.join(", ");
                format!(
                    "multiple versions of '{name}' installed ({list}); specify one with 'chum uninstall {name} <version>'"
                )
            }
            UserFacingError::RemoveFailed { path, source } => {
                format!("could not remove {}: {source}", path.display())
            }
            UserFacingError::DaemonUnreachable { path, source } => {
                format!(
                    "cannot reach chumd at {}: {source} (is chumd running?)",
                    path.display(),
                )
            }
            UserFacingError::DaemonProtocol { reason } => {
                format!("daemon protocol error: {reason}")
            }
            UserFacingError::ProcessNotInstalled { name, version: Some(v) } => {
                format!("'{name}' {v} is not installed")
            }
            UserFacingError::ProcessNotInstalled { name, version: None } => {
                format!("'{name}' is not installed")
            }
            UserFacingError::ProcessAlreadyRunning { name, version } => {
                format!("'{name}' {version} is already running (run 'chum stop {name}' first)")
            }
            UserFacingError::ProcessNotRunning { name, version } => {
                format!("'{name}' {version} is not running")
            }
            UserFacingError::ManifestMissing { install_dir } => {
                format!(
                    "chum-manifest.toml missing at {} (this install predates the v0.1 manifest-copy commit; re-install to repair)",
                    install_dir.display(),
                )
            }
            UserFacingError::LogsUnavailable { name, version, install_dir } => {
                format!(
                    "no logs yet for '{name}' {version} — start it once with 'chum start {name}' to populate {}/logs/",
                    install_dir.display(),
                )
            }
            UserFacingError::ServiceAlreadyInstalled { path } => {
                format!(
                    "LaunchAgent plist already exists at {} — re-run with --force to replace it",
                    path.display(),
                )
            }
            UserFacingError::ServiceCommandFailed { cmd, stderr } => {
                format!("'{cmd}' failed: {stderr}")
            }
            UserFacingError::PermissionDenied { name, version, unmet } => {
                let lines = unmet
                    .iter()
                    .map(|u| format!("    chum permit {name} --grant {u}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "'{name}' {version} needs grants not yet given:\n{lines}",
                )
            }
            UserFacingError::UnknownPermission { input } => {
                format!(
                    "'{input}' is not a known permission. Expected '<kind>=<value>' where kind is one of: filesystem.read, filesystem.write, network.outbound, env.read, subprocess.exec",
                )
            }
            UserFacingError::GrantNotFound { name, version, kind, value } => {
                format!("no grant '{kind}={value}' on '{name}' {version}")
            }
            UserFacingError::EnvKeyInvalid { key } => {
                format!("env key '{key}' is invalid")
            }
            UserFacingError::EnvUpdateFailed { path, reason } => {
                format!("failed to update {}: {reason}", path.display())
            }
        }
    }
}

/// Render an error to the appropriate stream.
///
/// Thin wrapper around [`render_actionable_error`]. Kept for source-
/// level backwards compat with earlier sessions' call sites; new code
/// should call `render_actionable_error` directly.
pub fn render(err: &UserFacingError, json: bool) {
    render_actionable_error(err, json);
}

/// Render an error with its recovery hint (when one exists) to the
/// appropriate stream.
///
/// - **Human mode** (`json = false`): two-line stderr output:
///   ```
///   error: <message>
///   hint:  <recovery hint>
///   ```
///   The `hint:` line is omitted when [`UserFacingError::recovery_hint`]
///   returns `None`. The process exits with code 1 from `main`.
///
/// - **JSON mode** (`json = true`): a single-object envelope on
///   stdout: `{ "status": "error", "code": "...", "message": "...",
///   "hint": "..." }`. The `"hint"` field is omitted when there's no
///   recovery hint.
pub fn render_actionable_error(err: &UserFacingError, json: bool) {
    let hint = err.recovery_hint();
    if json {
        let mut envelope = serde_json::json!({
            "status": "error",
            "code": err.code(),
            "message": err.message(),
        });
        if let Some(h) = hint {
            envelope["hint"] = serde_json::Value::String(h.to_string());
        }
        println!("{envelope}");
    } else {
        eprintln!("error: {}", err.message());
        if let Some(h) = hint {
            eprintln!("hint:  {h}");
        }
    }
}
