//! `chum` — the CHUM CLI.
//!
//! User-facing CLI. v0.1 composes
//! [`chum_core`] → [`chum_install`] → [`chum_registry`] directly because
//! `chum-daemon` does not exist yet; once the daemon protocol lands,
//! these calls move behind it (see `docs/ARCHITECTURE.md`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use clap::{Parser, Subcommand};

mod commands;
mod error;
mod output;

/// Top-level `chum` CLI invocation.
#[derive(Parser, Debug)]
#[command(
    name = "chum",
    version,
    about = "Local-first MCP package manager and capability broker."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Subcommands exposed by the CLI. v0.1 ships `install`, `list`,
/// `uninstall`, the process lifecycle group (`start` / `stop` /
/// `restart` / `status`), and the `daemon` diagnostic group; more
/// land in subsequent sessions.
#[derive(Subcommand, Debug)]
enum Command {
    /// Install an MCP server from a manifest TOML file.
    Install(commands::install::InstallArgs),
    /// List installed MCP servers recorded in the local registry.
    List(commands::list::ListArgs),
    /// Remove an installed MCP server's files and registry row.
    Uninstall(commands::uninstall::UninstallArgs),
    /// Ask the daemon to spawn an installed MCP server.
    Start(commands::start::StartArgs),
    /// Ask the daemon to terminate a running MCP server.
    Stop(commands::stop::StopArgs),
    /// Ask the daemon to stop and re-spawn a running MCP server.
    Restart(commands::restart::RestartArgs),
    /// Print the daemon-reported status of an installed MCP server.
    Status(commands::status_process::StatusProcessArgs),
    /// Tail recent log lines for an installed MCP server.
    Logs(commands::logs::LogsArgs),
    /// Grant capability permissions to an installed package.
    Permit(commands::permit::PermitArgs),
    /// Revoke a previously-granted permission.
    Revoke(commands::revoke::RevokeArgs),
    /// Show declared vs granted vs missing permissions.
    Permissions(commands::permissions::PermissionsArgs),
    /// Manage a package's env vars (set/unset/list).
    Env {
        #[command(subcommand)]
        sub: commands::env::EnvSub,
    },
    /// Diagnostic + control operations against the chumd daemon itself.
    Daemon {
        #[command(subcommand)]
        sub: commands::daemon::DaemonSub,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let (result, json) = match cli.command {
        Command::Install(args) => {
            let json = args.json;
            (commands::install::run(args).await, json)
        }
        Command::List(args) => {
            let json = args.json;
            (commands::list::run(args).await, json)
        }
        Command::Uninstall(args) => {
            let json = args.json;
            (commands::uninstall::run(args).await, json)
        }
        Command::Start(args) => {
            let json = args.json;
            (commands::start::run(args).await, json)
        }
        Command::Stop(args) => {
            let json = args.json;
            (commands::stop::run(args).await, json)
        }
        Command::Restart(args) => {
            let json = args.json;
            (commands::restart::run(args).await, json)
        }
        Command::Status(args) => {
            let json = args.json;
            (commands::status_process::run(args).await, json)
        }
        Command::Logs(args) => {
            let json = args.json;
            (commands::logs::run(args).await, json)
        }
        Command::Permit(args) => {
            let json = args.json;
            (commands::permit::run(args).await, json)
        }
        Command::Revoke(args) => {
            let json = args.json;
            (commands::revoke::run(args).await, json)
        }
        Command::Permissions(args) => {
            let json = args.json;
            (commands::permissions::run(args).await, json)
        }
        Command::Env { sub } => {
            let json = match &sub {
                commands::env::EnvSub::List(a) => a.json,
                commands::env::EnvSub::Set { common, .. } => common.json,
                commands::env::EnvSub::Unset { common, .. } => common.json,
            };
            (commands::env::run(sub).await, json)
        }
        Command::Daemon { sub } => {
            let json = match &sub {
                commands::daemon::DaemonSub::Ping(a) => a.json,
                commands::daemon::DaemonSub::Status(a) => a.json,
                commands::daemon::DaemonSub::InstallService(a) => a.json,
                commands::daemon::DaemonSub::UninstallService(a) => a.json,
                commands::daemon::DaemonSub::ServiceStatus(a) => a.json,
            };
            (commands::daemon::run(sub).await, json)
        }
    };
    if let Err(e) = result {
        error::render(&e, json);
        std::process::exit(1);
    }
}
