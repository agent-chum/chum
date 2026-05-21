//! `chum restart <name>` — ask the daemon to stop-and-respawn a
//! running package. Thin wrapper around
//! [`chum_daemon::DaemonClient::restart_process`].

use std::path::PathBuf;

use clap::Args;

use crate::commands::{map_lifecycle_ipc_error, resolve_lifecycle_target};
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum restart`.
#[derive(Args, Debug)]
pub struct RestartArgs {
    /// Package name to restart.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON on stdout.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum restart`.
pub async fn run(args: RestartArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let client = chum_daemon::DaemonClient::new(target.socket_path.clone());
    let resp = client
        .restart_process(&target.name, &target.version)
        .await
        .map_err(|e| map_lifecycle_ipc_error(e, &target))?;

    output::emit_restarted(&target.name, &target.version, &resp, args.json);
    Ok(())
}
