//! `chum start <name>` — ask the daemon to spawn an installed
//! package. Thin wrapper around [`chum_daemon::DaemonClient::spawn_process`].

use std::path::PathBuf;

use clap::Args;

use crate::commands::{map_lifecycle_ipc_error, resolve_lifecycle_target};
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum start`.
#[derive(Args, Debug)]
pub struct StartArgs {
    /// Package name to start.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path. Defaults to `<root>/daemon.sock`.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON on stdout instead of human text.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum start`.
pub async fn run(args: StartArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let client = chum_daemon::DaemonClient::new(target.socket_path.clone());
    let spawned = client
        .spawn_process(&target.name, &target.version)
        .await
        .map_err(|e| map_lifecycle_ipc_error(e, &target))?;

    output::emit_started(&target.name, &target.version, &spawned, args.json);
    Ok(())
}
