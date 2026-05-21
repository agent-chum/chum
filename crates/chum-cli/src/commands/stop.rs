//! `chum stop <name>` — ask the daemon to terminate a running
//! package. Thin wrapper around
//! [`chum_daemon::DaemonClient::terminate_process`].

use std::path::PathBuf;

use clap::Args;

use crate::commands::{map_lifecycle_ipc_error, resolve_lifecycle_target};
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum stop`.
#[derive(Args, Debug)]
pub struct StopArgs {
    /// Package name to stop.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Seconds to wait between SIGTERM and SIGKILL. Defaults to 5.
    #[arg(long)]
    pub grace: Option<u64>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path. Defaults to `<root>/daemon.sock`.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON on stdout.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum stop`.
pub async fn run(args: StopArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let client = chum_daemon::DaemonClient::new(target.socket_path.clone());
    let resp = client
        .terminate_process(&target.name, &target.version, args.grace)
        .await
        .map_err(|e| map_lifecycle_ipc_error(e, &target))?;

    output::emit_stopped(&target.name, &target.version, &resp, args.json);
    Ok(())
}
