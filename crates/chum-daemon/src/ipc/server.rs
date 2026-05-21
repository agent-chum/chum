//! IPC server: dispatch loop, per-connection handler, verb table.
//!
//! The server is invoked from `bin/chumd.rs` via [`serve`], which
//! takes ownership of a bound [`tokio::net::UnixListener`] and a
//! [`tokio::sync::watch::Receiver<bool>`] used to signal graceful
//! shutdown. The function returns when the shutdown signal flips
//! true and all in-flight handlers have drained (with a 5-second
//! hard ceiling).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::error::IpcError;
use crate::ipc::{
    DAEMON_VERSION, PROTOCOL_VERSION, ProcessKeyArgs, Request, Response, TerminateArgs, codes,
};
use crate::supervisor::{ProcessKey, ProcessStatus, Supervisor};

/// Hard size limit on a single request line. Defense-in-depth against
/// malformed clients; well above any v0.1 verb's actual payload.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Per-connection read timeout. If a client opens a connection and
/// sends nothing within this window, the handler closes the socket.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Hard ceiling on draining in-flight handlers during shutdown.
pub const SHUTDOWN_DRAIN_CEILING: Duration = Duration::from_secs(5);

/// State shared across all handler tasks. Cloned cheaply via `Arc`.
pub struct DaemonState {
    /// Wall-clock time at which the daemon process started — used
    /// to compute `ping.uptime_secs` and `status.started_at`.
    pub started_at: DateTime<Utc>,
    /// CHUM root for this daemon. The lifecycle verbs (spawn /
    /// terminate / restart / process_status) re-open the registry at
    /// `<root>/state.db` on every request — SQLite opens are
    /// microseconds, and per-verb opening avoids threading a shared
    /// rusqlite connection through tokio tasks.
    pub root: PathBuf,
    /// Snapshot of registered packages at daemon startup. Read once
    /// from the registry; not refreshed during the daemon's lifetime
    /// in v0.1 (Session B introduces refresh on install/uninstall).
    pub installed_count: u32,
    /// Process supervisor. Lifecycle verbs drive this; v0.1 never
    /// pre-populates it on startup (Session B introduces auto-start
    /// based on registry rows).
    pub supervisor: Supervisor,
    /// User-driven restart counter per process key. Incremented by
    /// the `restart` verb, reset to 0 by `spawn`, removed by
    /// `terminate`. Distinct from the supervisor's internal
    /// `restart_count` which counts policy-driven respawns inside
    /// the monitor task.
    pub restart_counts: std::sync::Mutex<HashMap<ProcessKey, u32>>,
}

impl DaemonState {
    /// Convenience constructor. Builds a default `Supervisor` and an
    /// empty user-restart-count map.
    pub fn new(started_at: DateTime<Utc>, root: PathBuf, installed_count: u32) -> Self {
        Self {
            started_at,
            root,
            installed_count,
            supervisor: Supervisor::new(),
            restart_counts: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

/// Accept loop. Runs until `shutdown_rx` flips to `true`, then drains
/// in-flight handlers up to [`SHUTDOWN_DRAIN_CEILING`] and returns.
///
/// `listener` is consumed: callers are responsible for removing the
/// socket file after `serve` returns (the v0.1 binary does this
/// inside `main`).
pub async fn serve(
    listener: UnixListener,
    state: Arc<DaemonState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), IpcError> {
    let mut tasks: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            biased;
            // Shutdown signal takes priority over accepting new connections.
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let state = state.clone();
                        tasks.spawn(handle_connection(stream, state));
                    }
                    Err(e) => {
                        eprintln!(
                            "{} chumd: accept failed: {e}",
                            Utc::now().to_rfc3339(),
                        );
                    }
                }
            }
        }
    }

    let drain = async {
        while tasks.join_next().await.is_some() {}
    };
    let _ = tokio::time::timeout(SHUTDOWN_DRAIN_CEILING, drain).await;
    Ok(())
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: Arc<DaemonState>,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let mut buf = Vec::with_capacity(1024);
    let read_result = tokio::time::timeout(
        READ_TIMEOUT,
        reader.read_until(b'\n', &mut buf),
    )
    .await;

    let response = match read_result {
        Err(_) => Response::error(
            codes::REQUEST_TIMEOUT,
            format!("client idle for {READ_TIMEOUT:?}"),
        ),
        Ok(Err(e)) => Response::error(codes::INVALID_REQUEST, e.to_string()),
        Ok(Ok(0)) => Response::error(codes::INVALID_REQUEST, "empty request"),
        Ok(Ok(n)) if n > MAX_REQUEST_BYTES => Response::error(
            codes::REQUEST_TOO_LARGE,
            format!("request was {n} bytes; limit is {MAX_REQUEST_BYTES}"),
        ),
        Ok(Ok(_)) => dispatch_bytes(&buf, &state).await,
    };

    let body = match serde_json::to_vec(&response) {
        Ok(b) => b,
        Err(_) => br#"{"protocol_version":1,"status":"error","code":"internal","message":"response serialization failed"}"#.to_vec(),
    };
    let _ = write_half.write_all(&body).await;
    let _ = write_half.write_all(b"\n").await;
    let _ = write_half.shutdown().await;
}

async fn dispatch_bytes(buf: &[u8], state: &Arc<DaemonState>) -> Response {
    let req: Request = match serde_json::from_slice(buf) {
        Ok(r) => r,
        Err(e) => {
            return Response::error(codes::INVALID_REQUEST, format!("invalid request JSON: {e}"));
        }
    };
    if req.protocol_version != PROTOCOL_VERSION {
        return Response::error(
            codes::UNSUPPORTED_PROTOCOL_VERSION,
            format!(
                "expected protocol_version {PROTOCOL_VERSION}, got {}",
                req.protocol_version,
            ),
        );
    }
    match req.verb.as_str() {
        "ping" => verb_ping(state).await,
        "status" => verb_status(state).await,
        "list_processes" => verb_list_processes(state).await,
        "spawn" => verb_spawn(state, req.args).await,
        "terminate" => verb_terminate(state, req.args).await,
        "restart" => verb_restart(state, req.args).await,
        "process_status" => verb_process_status(state, req.args).await,
        other => Response::error(
            codes::UNKNOWN_VERB,
            format!("verb '{other}' is not recognised by this daemon"),
        ),
    }
}

async fn verb_ping(state: &Arc<DaemonState>) -> Response {
    let uptime = (Utc::now() - state.started_at)
        .num_seconds()
        .max(0) as u64;
    Response::ok(serde_json::json!({
        "daemon_version": DAEMON_VERSION,
        "uptime_secs": uptime,
        "installed_count": state.installed_count,
    }))
}

async fn verb_status(state: &Arc<DaemonState>) -> Response {
    let running = state.supervisor.list().await.len() as u32;
    Response::ok(serde_json::json!({
        "pid": std::process::id(),
        "started_at": state.started_at.to_rfc3339(),
        "installed_count": state.installed_count,
        "running_count": running,
    }))
}

async fn verb_list_processes(state: &Arc<DaemonState>) -> Response {
    let entries = state.supervisor.list().await;
    let counts_snapshot: HashMap<ProcessKey, u32> = state
        .restart_counts
        .lock()
        .map(|m| m.clone())
        .unwrap_or_default();

    let mut processes: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
    for (key, status) in entries {
        let pid = state.supervisor.pid(&key).await;
        let restart_count = counts_snapshot.get(&key).copied().unwrap_or(0);
        let (status_str, exit_code) = status_string_and_exit(&status);
        let mut entry = serde_json::json!({
            "name": key.name,
            "version": key.version,
            "status": status_str,
            "restart_count": restart_count,
        });
        if let Some(p) = pid {
            entry["pid"] = serde_json::json!(p);
        }
        if let Some(c) = exit_code {
            entry["exit_code"] = serde_json::json!(c);
        }
        processes.push(entry);
    }
    Response::ok(serde_json::json!({ "processes": processes }))
}

async fn verb_spawn(state: &Arc<DaemonState>, args: serde_json::Value) -> Response {
    let parsed: ProcessKeyArgs = match serde_json::from_value(args) {
        Ok(p) => p,
        Err(e) => return Response::error(codes::INVALID_REQUEST, format!("args: {e}")),
    };

    let artifact = match resolve_artifact(state, &parsed.name, &parsed.version) {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    let manifest = match read_manifest(&artifact.install_dir) {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let key = ProcessKey::new(parsed.name.clone(), parsed.version.clone());
    let handle = match state.supervisor.spawn(artifact, manifest).await {
        Ok(h) => h,
        Err(crate::SupervisorError::AlreadyRunning { .. }) => {
            return Response::error(
                codes::PROCESS_ALREADY_RUNNING,
                format!("'{}' {} is already running", parsed.name, parsed.version),
            );
        }
        Err(crate::SupervisorError::SpawnFailed { source }) => {
            return Response::error(codes::SPAWN_FAILED, source.to_string());
        }
        Err(e) => return Response::error(codes::INTERNAL, e.to_string()),
    };

    if let Ok(mut counts) = state.restart_counts.lock() {
        counts.insert(key, 0);
    }

    Response::ok(serde_json::json!({
        "pid": handle.pid,
        "started_at": handle.started_at.to_rfc3339(),
    }))
}

async fn verb_terminate(state: &Arc<DaemonState>, args: serde_json::Value) -> Response {
    let parsed: TerminateArgs = match serde_json::from_value(args) {
        Ok(p) => p,
        Err(e) => return Response::error(codes::INVALID_REQUEST, format!("args: {e}")),
    };
    let key = ProcessKey::new(parsed.name.clone(), parsed.version.clone());
    let grace = Duration::from_secs(parsed.grace_secs.unwrap_or(5));

    match state.supervisor.stop(&key, grace).await {
        Ok(()) => {
            if let Ok(mut counts) = state.restart_counts.lock() {
                counts.remove(&key);
            }
            Response::ok(serde_json::json!({ "stopped": true }))
        }
        Err(crate::SupervisorError::NotRunning { .. }) => Response::error(
            codes::PROCESS_NOT_RUNNING,
            format!("'{}' {} is not running", parsed.name, parsed.version),
        ),
        Err(crate::SupervisorError::KillFailed { reason }) => {
            Response::error(codes::KILL_FAILED, reason)
        }
        Err(crate::SupervisorError::MonitorWedged { .. }) => Response::error(
            codes::MONITOR_WEDGED,
            format!(
                "supervisor monitor for '{}' {} ended unexpectedly",
                parsed.name, parsed.version
            ),
        ),
        Err(e) => Response::error(codes::INTERNAL, e.to_string()),
    }
}

async fn verb_restart(state: &Arc<DaemonState>, args: serde_json::Value) -> Response {
    let parsed: ProcessKeyArgs = match serde_json::from_value(args) {
        Ok(p) => p,
        Err(e) => return Response::error(codes::INVALID_REQUEST, format!("args: {e}")),
    };
    let key = ProcessKey::new(parsed.name.clone(), parsed.version.clone());

    let handle = match state.supervisor.restart(&key).await {
        Ok(h) => h,
        Err(crate::SupervisorError::NotRunning { .. }) => {
            return Response::error(
                codes::PROCESS_NOT_RUNNING,
                format!("'{}' {} is not running", parsed.name, parsed.version),
            );
        }
        Err(crate::SupervisorError::SpawnFailed { source }) => {
            return Response::error(codes::SPAWN_FAILED, source.to_string());
        }
        Err(e) => return Response::error(codes::INTERNAL, e.to_string()),
    };

    let count = if let Ok(mut counts) = state.restart_counts.lock() {
        let c = counts.entry(key).or_insert(0);
        *c += 1;
        *c
    } else {
        0
    };

    Response::ok(serde_json::json!({
        "pid": handle.pid,
        "started_at": handle.started_at.to_rfc3339(),
        "restart_count": count,
    }))
}

async fn verb_process_status(state: &Arc<DaemonState>, args: serde_json::Value) -> Response {
    let parsed: ProcessKeyArgs = match serde_json::from_value(args) {
        Ok(p) => p,
        Err(e) => return Response::error(codes::INVALID_REQUEST, format!("args: {e}")),
    };
    let key = ProcessKey::new(parsed.name.clone(), parsed.version.clone());

    // Registry check first — if the process isn't installed, that's
    // the more useful error to surface.
    if let Err(resp) = registry_must_contain(state, &parsed.name, &parsed.version) {
        return resp;
    }

    let status = state.supervisor.status(&key).await;
    let pid = state.supervisor.pid(&key).await;
    let restart_count = state
        .restart_counts
        .lock()
        .ok()
        .and_then(|c| c.get(&key).copied())
        .unwrap_or(0);

    let (status_str, exit_code) = match status.as_ref() {
        Some(s) => status_string_and_exit(s),
        // Registry has the row but supervisor doesn't — process was
        // installed but never spawned in this daemon's lifetime.
        None => ("stopped", None),
    };

    let mut data = serde_json::json!({
        "name": parsed.name,
        "version": parsed.version,
        "status": status_str,
        "restart_count": restart_count,
    });
    if let Some(p) = pid {
        data["pid"] = serde_json::json!(p);
    }
    if let Some(c) = exit_code {
        data["exit_code"] = serde_json::json!(c);
    }
    Response::ok(data)
}

fn status_string_and_exit(s: &ProcessStatus) -> (&'static str, Option<i32>) {
    match s {
        ProcessStatus::Starting => ("starting", None),
        ProcessStatus::Running => ("running", None),
        ProcessStatus::Restarting => ("restarting", None),
        ProcessStatus::Stopped => ("stopped", None),
        ProcessStatus::Failed { exit_code } => ("failed", Some(*exit_code)),
    }
}

/// Open the registry and look up `(name, version)`. On NotFound, returns
/// the canonical `process_not_installed` error response.
fn resolve_artifact(
    state: &Arc<DaemonState>,
    name: &str,
    version: &str,
) -> Result<chum_install::InstalledArtifact, Response> {
    let db = state.root.join("state.db");
    if !db.is_file() {
        return Err(Response::error(
            codes::PROCESS_NOT_INSTALLED,
            format!("'{name}' {version} is not installed (no registry file)"),
        ));
    }
    let registry = chum_registry::Registry::open(&db).map_err(|e| {
        Response::error(codes::INTERNAL, format!("registry open failed: {e}"))
    })?;
    match registry.get_by_name_version(name, version) {
        Ok(row) => Ok(row.into()),
        Err(chum_registry::RegistryError::NotFound { .. }) => Err(Response::error(
            codes::PROCESS_NOT_INSTALLED,
            format!("'{name}' {version} is not installed"),
        )),
        Err(e) => Err(Response::error(codes::INTERNAL, e.to_string())),
    }
}

/// Lighter version of `resolve_artifact` for verbs that need only to
/// know whether the row exists.
fn registry_must_contain(
    state: &Arc<DaemonState>,
    name: &str,
    version: &str,
) -> Result<(), Response> {
    let _ = resolve_artifact(state, name, version)?;
    Ok(())
}

/// Read + parse `<install_dir>/chum-manifest.toml`. NotFound surfaces
/// as the `manifest_missing_in_install_dir` code.
fn read_manifest(install_dir: &std::path::Path) -> Result<chum_core::Manifest, Response> {
    let path = install_dir.join("chum-manifest.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Response::error(
                codes::MANIFEST_MISSING_IN_INSTALL_DIR,
                format!(
                    "chum-manifest.toml missing at {} (re-install to populate)",
                    install_dir.display()
                ),
            ));
        }
        Err(e) => {
            return Err(Response::error(
                codes::INTERNAL,
                format!("read manifest: {e}"),
            ));
        }
    };
    chum_core::parse_and_validate(&text)
        .map_err(|e| Response::error(codes::MANIFEST_INVALID, e.to_string()))
}
