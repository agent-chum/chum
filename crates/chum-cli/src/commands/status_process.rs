//! `chum status <name>` — print the daemon-reported lifecycle status
//! for an installed package. Distinct from `chum daemon status` (which
//! prints the daemon process's own status).

use std::path::PathBuf;

use clap::Args;

use crate::commands::{map_lifecycle_ipc_error, resolve_lifecycle_target};
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum status`.
#[derive(Args, Debug)]
pub struct StatusProcessArgs {
    /// Package name to query.
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

    /// Disable ANSI color escapes even when stdout is a tty.
    #[arg(long)]
    pub no_color: bool,
}

/// Execute `chum status`.
pub async fn run(args: StatusProcessArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let client = chum_daemon::DaemonClient::new(target.socket_path.clone());
    let resp = client
        .process_status(&target.name, &target.version)
        .await
        .map_err(|e| map_lifecycle_ipc_error(e, &target))?;

    output::emit_process_status(&resp, args.json, args.no_color);
    Ok(())
}
