//! Subcommand implementations.
//!
//! Each module owns one subcommand's pipeline. The top-level `main.rs`
//! dispatches into these by matching on the clap `Command` enum.
//!
//! Shared helpers (like [`resolve_root`]) live here so individual
//! subcommands stay focused on their own pipeline.

use std::path::PathBuf;

use crate::error::UserFacingError;

pub mod daemon;
pub mod daemon_service;
pub mod env;
pub mod install;
pub mod list;
pub mod logs;
pub mod permissions;
pub mod permit;
pub mod restart;
pub mod revoke;
pub mod search;
pub mod start;
pub mod status_process;
pub mod stop;
pub mod uninstall;

/// Resolve the CHUM root directory for this invocation.
///
/// Returns `arg` verbatim if a `--root` override was supplied. Otherwise
/// calls [`chum_install::chum_home`] and maps the "no env vars set" case
/// (manifesting as `InstallError::Io` with `ErrorKind::NotFound`) into
/// the clean [`UserFacingError::ChumHomeUnresolved`] code, so callers do
/// not leak the `install_io` code for what is really a configuration
/// gap.
pub(crate) fn resolve_root(arg: Option<PathBuf>) -> Result<PathBuf, UserFacingError> {
    if let Some(r) = arg {
        return Ok(r);
    }
    chum_install::chum_home().map_err(|e| match e {
        chum_install::InstallError::Io(io)
            if io.kind() == std::io::ErrorKind::NotFound =>
        {
            UserFacingError::ChumHomeUnresolved
        }
        other => UserFacingError::Install(other),
    })
}

/// Resolved target for the four lifecycle subcommands (`start`,
/// `stop`, `restart`, `status`). Carries everything they need so the
/// per-subcommand `run()` is a thin wrapper around the `DaemonClient`
/// call.
pub(crate) struct LifecycleTarget {
    pub name: String,
    pub version: String,
    pub install_dir: std::path::PathBuf,
    pub socket_path: std::path::PathBuf,
}

/// Resolve `(name, version, install_dir, socket_path)` for a
/// lifecycle subcommand.
///
/// - If `version_arg` is supplied, takes it verbatim and pulls
///   `install_dir` straight from the registry row (NotFound →
///   `process_not_installed`).
/// - If `version_arg` is `None`, calls `list_by_name(name)`:
///   - 0 rows → `process_not_installed`
///   - 1 row → use it
///   - 2+ rows → `ambiguous_version` listing all versions
///
/// `socket_path` defaults to `<root>/daemon.sock` unless
/// `socket_path_arg` overrides.
pub(crate) fn resolve_lifecycle_target(
    name: &str,
    version_arg: Option<&str>,
    root_arg: Option<PathBuf>,
    socket_path_arg: Option<PathBuf>,
) -> Result<LifecycleTarget, UserFacingError> {
    let root = resolve_root(root_arg)?;
    let socket_path = socket_path_arg.unwrap_or_else(|| root.join("daemon.sock"));

    let db = root.join("state.db");
    if !db.is_file() {
        return Err(UserFacingError::ProcessNotInstalled {
            name: name.to_string(),
            version: version_arg.map(|s| s.to_string()),
        });
    }
    let registry =
        chum_registry::Registry::open(&db).map_err(UserFacingError::Registry)?;

    let (version, install_dir) = match version_arg {
        Some(v) => match registry.get_by_name_version(name, v) {
            Ok(row) => (row.version, row.install_dir),
            Err(chum_registry::RegistryError::NotFound { .. }) => {
                return Err(UserFacingError::ProcessNotInstalled {
                    name: name.to_string(),
                    version: Some(v.to_string()),
                });
            }
            Err(e) => return Err(UserFacingError::Registry(e)),
        },
        None => {
            let mut matches = registry
                .list_by_name(name)
                .map_err(UserFacingError::Registry)?;
            match matches.len() {
                0 => {
                    return Err(UserFacingError::ProcessNotInstalled {
                        name: name.to_string(),
                        version: None,
                    });
                }
                1 => {
                    let row = matches.swap_remove(0);
                    (row.version, row.install_dir)
                }
                _ => {
                    let versions =
                        matches.into_iter().map(|r| r.version).collect();
                    return Err(UserFacingError::AmbiguousVersion {
                        name: name.to_string(),
                        versions,
                    });
                }
            }
        }
    };

    Ok(LifecycleTarget {
        name: name.to_string(),
        version,
        install_dir,
        socket_path,
    })
}

/// Map an [`chum_daemon::IpcError`] from a lifecycle IPC call into
/// the cli's [`UserFacingError`] envelope, translating the
/// daemon-side wire codes into typed variants where possible.
pub(crate) fn map_lifecycle_ipc_error(
    err: chum_daemon::IpcError,
    target: &LifecycleTarget,
) -> UserFacingError {
    use chum_daemon::IpcError;
    match err {
        IpcError::ConnectFailed { path, source } => {
            UserFacingError::DaemonUnreachable { path, source }
        }
        IpcError::Io(source) => UserFacingError::DaemonUnreachable {
            path: target.socket_path.clone(),
            source,
        },
        IpcError::ProtocolError { reason } => UserFacingError::DaemonProtocol { reason },
        IpcError::Json(e) => UserFacingError::DaemonProtocol {
            reason: format!("json decode failed: {e}"),
        },
        IpcError::ServerError { code, message } => match code.as_str() {
            chum_daemon::codes::PROCESS_NOT_INSTALLED => {
                UserFacingError::ProcessNotInstalled {
                    name: target.name.clone(),
                    version: Some(target.version.clone()),
                }
            }
            chum_daemon::codes::PROCESS_ALREADY_RUNNING => {
                UserFacingError::ProcessAlreadyRunning {
                    name: target.name.clone(),
                    version: target.version.clone(),
                }
            }
            chum_daemon::codes::PROCESS_NOT_RUNNING => {
                UserFacingError::ProcessNotRunning {
                    name: target.name.clone(),
                    version: target.version.clone(),
                }
            }
            chum_daemon::codes::MANIFEST_MISSING_IN_INSTALL_DIR => {
                UserFacingError::ManifestMissing {
                    install_dir: target.install_dir.clone(),
                }
            }
            chum_daemon::codes::LOGS_UNAVAILABLE => UserFacingError::LogsUnavailable {
                name: target.name.clone(),
                version: target.version.clone(),
                install_dir: target.install_dir.clone(),
            },
            chum_daemon::codes::PERMISSION_DENIED => UserFacingError::PermissionDenied {
                name: target.name.clone(),
                version: target.version.clone(),
                unmet: parse_unmet_grants_from_message(&message),
            },
            other => UserFacingError::DaemonProtocol {
                reason: format!("server error {other}: {message}"),
            },
        },
        IpcError::BindFailed { .. } | IpcError::SocketAlreadyInUse { .. } => {
            UserFacingError::DaemonProtocol {
                reason: format!("unexpected server-side error from client path: {err:?}"),
            }
        }
    }
}

/// Extract the comma-separated `kind=value` list out of a daemon
/// `permission_denied` message. The daemon's exact format is:
/// `'<name>' <version> requires grants not yet given: k1=v1, k2=v2. Run: chum permit ...`
///
/// We isolate the segment between "given: " and ". Run:" then split
/// on ", ". Best-effort — if the daemon's message changes shape in a
/// later session, the cli still surfaces a useful error (the raw
/// message), just without the structured `unmet` field.
fn parse_unmet_grants_from_message(msg: &str) -> Vec<String> {
    let Some(after) = msg.split_once("given: ") else {
        return Vec::new();
    };
    let payload = after.1.split_once(". Run:").map(|p| p.0).unwrap_or(after.1);
    payload
        .split(", ")
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .collect()
}
