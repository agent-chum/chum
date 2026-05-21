//! `chum daemon` — diagnostic subcommands talking to the running
//! `chumd` over the IPC socket.
//!
//! v0.1 verbs: `ping` and `status`. `list_processes` exists in the
//! protocol but is not exposed via cli until Session B introduces
//! actual running processes — running an empty list every session
//! would be noise.

use std::path::PathBuf;

use chum_daemon::{DaemonClient, IpcError};
use clap::{Args, Subcommand};

use crate::error::UserFacingError;
use crate::output;

/// Arguments shared by every `chum daemon <sub>` invocation.
#[derive(Args, Debug, Clone)]
pub struct DaemonCommonArgs {
    /// Override the IPC socket path. Default: `<root>/daemon.sock`.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,

    /// Override CHUM_HOME for this invocation (used to derive the
    /// default socket path when `--socket-path` is not given).
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Emit machine-readable JSON on stdout instead of human text.
    #[arg(long)]
    pub json: bool,
}

/// `chum daemon <sub>` subcommands.
#[derive(Subcommand, Debug)]
pub enum DaemonSub {
    /// Send a ping to the daemon. Fast path for "is chumd reachable?"
    Ping(DaemonCommonArgs),
    /// Print the daemon's status snapshot.
    Status(DaemonCommonArgs),
}

/// Top-level dispatch for `chum daemon`.
pub async fn run(sub: DaemonSub) -> Result<(), UserFacingError> {
    match sub {
        DaemonSub::Ping(args) => ping(args).await,
        DaemonSub::Status(args) => status(args).await,
    }
}

async fn ping(args: DaemonCommonArgs) -> Result<(), UserFacingError> {
    let socket = resolve_socket_path(&args)?;
    let client = DaemonClient::new(socket.clone());
    let resp = client
        .ping()
        .await
        .map_err(|e| map_ipc_error(e, socket.clone()))?;
    output::emit_daemon_ping(&resp, args.json);
    Ok(())
}

async fn status(args: DaemonCommonArgs) -> Result<(), UserFacingError> {
    let socket = resolve_socket_path(&args)?;
    let client = DaemonClient::new(socket.clone());
    let resp = client
        .status()
        .await
        .map_err(|e| map_ipc_error(e, socket.clone()))?;
    output::emit_daemon_status(&resp, args.json);
    Ok(())
}

fn resolve_socket_path(args: &DaemonCommonArgs) -> Result<PathBuf, UserFacingError> {
    if let Some(p) = &args.socket_path {
        return Ok(p.clone());
    }
    let root = crate::commands::resolve_root(args.root.clone())?;
    Ok(root.join("daemon.sock"))
}

/// Map an [`IpcError`] from the daemon client into the cli's
/// [`UserFacingError`] envelope so the renderer sees a stable code.
///
/// The client only emits a few of the `IpcError` variants in
/// practice (`ConnectFailed`, `ProtocolError`, `ServerError`,
/// `Json`, `Io`). The server-side variants (`BindFailed`,
/// `SocketAlreadyInUse`) are matched here as defense-in-depth — if
/// they ever surface client-side it would be a bug in the library,
/// and we map them to a generic protocol error rather than panic.
fn map_ipc_error(err: IpcError, socket_path: PathBuf) -> UserFacingError {
    match err {
        IpcError::ConnectFailed { path, source } => {
            UserFacingError::DaemonUnreachable { path, source }
        }
        IpcError::Io(source) => UserFacingError::DaemonUnreachable {
            path: socket_path,
            source,
        },
        IpcError::ProtocolError { reason } => UserFacingError::DaemonProtocol { reason },
        IpcError::Json(e) => UserFacingError::DaemonProtocol {
            reason: format!("json decode failed: {e}"),
        },
        IpcError::ServerError { code, message } => UserFacingError::DaemonProtocol {
            reason: format!("server returned error {code}: {message}"),
        },
        IpcError::BindFailed { path, source } => UserFacingError::DaemonProtocol {
            reason: format!(
                "unexpected server-side BindFailed surfaced to client: {} ({source})",
                path.display(),
            ),
        },
        IpcError::SocketAlreadyInUse { path } => UserFacingError::DaemonProtocol {
            reason: format!(
                "unexpected server-side SocketAlreadyInUse surfaced to client: {}",
                path.display(),
            ),
        },
    }
}
