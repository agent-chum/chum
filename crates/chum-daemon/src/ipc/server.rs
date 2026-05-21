//! IPC server: dispatch loop, per-connection handler, verb table.
//!
//! The server is invoked from `bin/chumd.rs` via [`serve`], which
//! takes ownership of a bound [`tokio::net::UnixListener`] and a
//! [`tokio::sync::watch::Receiver<bool>`] used to signal graceful
//! shutdown. The function returns when the shutdown signal flips
//! true and all in-flight handlers have drained (with a 5-second
//! hard ceiling).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::error::IpcError;
use crate::ipc::{
    DAEMON_VERSION, PROTOCOL_VERSION, Request, Response, codes,
};
use crate::supervisor::Supervisor;

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
    /// Snapshot of registered packages at daemon startup. Read once
    /// from the registry; not refreshed during the daemon's lifetime
    /// in v0.1 (Session B introduces refresh on install/uninstall).
    pub installed_count: u32,
    /// Process supervisor. v0.1 never spawns into it; it is reserved
    /// here so `list_processes` has a uniform source of truth that
    /// Session B can populate without restructuring `DaemonState`.
    pub supervisor: Supervisor,
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
    let processes: Vec<serde_json::Value> = state
        .supervisor
        .list()
        .await
        .into_iter()
        .map(|(key, status)| {
            serde_json::json!({
                "name": key.name,
                "version": key.version,
                "status": format!("{status:?}"),
            })
        })
        .collect();
    Response::ok(serde_json::json!({ "processes": processes }))
}
