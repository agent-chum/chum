//! `chumd` — CHUM background daemon binary.
//!
//! v0.1 scope: bind a Unix domain socket at `<chum_home>/daemon.sock`,
//! serve the IPC verbs defined in `chum_daemon::ipc::server`, and
//! shut down cleanly on SIGTERM / SIGINT.
//!
//! Future work (Session B / Session C):
//! - `chum start <name>` plumbing into the supervisor.
//! - launchd integration so chumd auto-starts on user login.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use chum_daemon::{IpcError, Supervisor};
use chum_daemon::ipc::server::{DaemonState, serve};
use clap::Parser;
use thiserror::Error;
use tokio::net::UnixListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

/// Command-line arguments for `chumd`.
#[derive(Parser, Debug)]
#[command(
    name = "chumd",
    version,
    about = "CHUM background daemon. Bind a Unix socket and serve the IPC protocol."
)]
struct Args {
    /// Override the CHUM root directory. Defaults to `chum_home()`
    /// (`$CHUM_HOME` → `$XDG_DATA_HOME/chum` → `$HOME/.chum`).
    #[arg(long)]
    root: Option<PathBuf>,

    /// Override the IPC socket path. Defaults to `<root>/daemon.sock`.
    #[arg(long)]
    socket_path: Option<PathBuf>,
}

/// Errors that can fail `chumd` before, during, or after the accept
/// loop. Renders human messages via `Display` (thiserror).
#[derive(Debug, Error)]
enum ChumdError {
    /// `chum_home()` resolution failed.
    #[error("cannot resolve CHUM root: {0}")]
    Root(#[from] chum_install::InstallError),

    /// Reading the install_artifacts table failed at startup.
    #[error("registry error: {0}")]
    Registry(#[from] chum_registry::RegistryError),

    /// Filesystem error preparing the root directory or the socket
    /// file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Server-side IPC failure (bind, accept, or in-flight handler
    /// fault).
    #[error("ipc: {0}")]
    Ipc(#[from] IpcError),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), ChumdError> {
    let args = Args::parse();

    let root = match args.root {
        Some(r) => r,
        None => chum_install::chum_home()?,
    };
    std::fs::create_dir_all(&root)?;
    let socket_path = args
        .socket_path
        .unwrap_or_else(|| root.join("daemon.sock"));

    check_or_remove_zombie(&socket_path)?;
    let listener = UnixListener::bind(&socket_path).map_err(|source| IpcError::BindFailed {
        path: socket_path.clone(),
        source,
    })?;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(&socket_path, perms)?;

    let installed_count = read_installed_count(&root)?;

    let state = Arc::new(DaemonState {
        started_at: Utc::now(),
        installed_count,
        supervisor: Supervisor::new(),
    });

    log(&format!(
        "chumd ready (socket={}, installed_count={installed_count})",
        socket_path.display(),
    ));

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_tx_for_signals = shutdown_tx.clone();
    tokio::spawn(async move {
        watch_for_shutdown_signal(shutdown_tx_for_signals).await;
    });

    let serve_result = serve(listener, state, shutdown_rx).await;

    let _ = std::fs::remove_file(&socket_path);
    log("chumd stopped");

    serve_result?;
    Ok(())
}

/// Listen for SIGTERM / SIGINT. On either, flip the shutdown watch
/// channel so `serve()` can drain handlers and return.
async fn watch_for_shutdown_signal(tx: watch::Sender<bool>) {
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("failed to install SIGTERM handler: {e}"));
            return;
        }
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("failed to install SIGINT handler: {e}"));
            return;
        }
    };
    tokio::select! {
        _ = sigterm.recv() => log("SIGTERM received"),
        _ = sigint.recv()  => log("SIGINT received"),
    }
    let _ = tx.send(true);
}

/// One-line stderr logger with an RFC3339 timestamp prefix.
fn log(msg: &str) {
    eprintln!("{} chumd: {msg}", Utc::now().to_rfc3339());
}

/// Decide whether an existing path at the socket location is a live
/// chumd (refuse) or a stale leftover (remove + continue).
///
/// Uses the synchronous `std::os::unix::net::UnixStream::connect`
/// rather than tokio's async version so the check works before the
/// runtime is up.
fn check_or_remove_zombie(path: &Path) -> Result<(), ChumdError> {
    if !path.exists() {
        return Ok(());
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Err(ChumdError::Ipc(IpcError::SocketAlreadyInUse {
            path: path.to_path_buf(),
        })),
        Err(_) => {
            std::fs::remove_file(path)?;
            Ok(())
        }
    }
}

/// Count installed packages by reading the registry once at startup.
///
/// Missing `state.db` is treated as "no packages installed" — chumd
/// starts cleanly on a fresh machine without creating the database.
fn read_installed_count(root: &Path) -> Result<u32, ChumdError> {
    let db = root.join("state.db");
    if !db.is_file() {
        return Ok(0);
    }
    let registry = chum_registry::Registry::open(&db)?;
    let rows = registry.list_all()?;
    Ok(rows.len() as u32)
}
