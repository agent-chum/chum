//! `chum logs <name>` — print the last N lines of an installed
//! package's stdout / stderr / both. Thin wrapper around
//! [`chum_daemon::DaemonClient::tail_logs`].
//!
// TODO(chum-v0.2): add `--follow` / streaming. Requires a long-lived
// IPC connection — outside v0.1's single-req-per-connection model.

use std::path::PathBuf;

use clap::Args;

use crate::commands::{map_lifecycle_ipc_error, resolve_lifecycle_target};
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum logs`.
#[derive(Args, Debug)]
pub struct LogsArgs {
    /// Package name to read logs for.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Number of lines to return per stream. Defaults to 100; the
    /// daemon caps requests at 10,000.
    #[arg(long, default_value_t = 100)]
    pub lines: usize,
    /// Show stdout only.
    #[arg(long, conflicts_with = "stderr")]
    pub stdout: bool,
    /// Show stderr only.
    #[arg(long, conflicts_with = "stdout")]
    pub stderr: bool,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON envelope on stdout instead of
    /// the raw log content.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum logs`.
pub async fn run(args: LogsArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let stream = if args.stdout {
        "stdout"
    } else if args.stderr {
        "stderr"
    } else {
        "both"
    };

    let client = chum_daemon::DaemonClient::new(target.socket_path.clone());
    let resp = client
        .tail_logs(&target.name, &target.version, stream, args.lines)
        .await
        .map_err(|e| map_lifecycle_ipc_error(e, &target))?;

    output::emit_logs(&resp, args.json);
    Ok(())
}
