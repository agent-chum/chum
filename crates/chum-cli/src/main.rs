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
mod term;

/// Top-level `chum` CLI invocation.
#[derive(Parser, Debug)]
#[command(
    name = "chum",
    version,
    about = "Local-first MCP package manager and capability broker.",
    long_about = "Local-first MCP package manager and capability broker.\n\nInstalls, supervises, and capability-gates Model Context Protocol servers as packages. Pairs with the chumd background daemon for process lifecycle.\n\nEXAMPLES:\n  chum install manifests/chum-everything.toml --root /tmp/demo\n  chum start everything --root /tmp/demo\n  chum list --root /tmp/demo\n  chum logs everything --lines 50 --root /tmp/demo\n\nRun `chum help <COMMAND>` for per-command details + examples."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Subcommands exposed by the CLI.
#[derive(Subcommand, Debug)]
enum Command {
    /// Install an MCP server from a manifest TOML file.
    ///
    /// Reads + validates the manifest, runs the source-specific
    /// install (npm subprocess / local symlink / binary fetch +
    /// extract), and records the row in the local registry.
    ///
    /// EXAMPLES:
    ///   chum install manifests/chum-everything.toml --root /tmp/demo
    ///   chum install manifests/chum-brave-search.toml --root /tmp/demo --json
    ///   chum install <manifest.toml> --dry-run     # parse + validate only
    Install(commands::install::InstallArgs),

    /// List installed MCP servers recorded in the local registry.
    ///
    /// EXAMPLES:
    ///   chum list --root /tmp/demo
    ///   chum list chum- --root /tmp/demo    # name-prefix filter
    ///   chum list --root /tmp/demo --json
    List(commands::list::ListArgs),

    /// Remove an installed MCP server's files and registry row.
    ///
    /// Defaults to a y/N confirmation prompt unless --force, --json,
    /// or stdin is not a tty. --keep-files removes only the registry
    /// row, leaving install_dir on disk.
    ///
    /// EXAMPLES:
    ///   chum uninstall everything --root /tmp/demo --force
    ///   chum uninstall everything 0.1.0 --root /tmp/demo --force
    ///   chum uninstall everything --keep-files --json --root /tmp/demo
    Uninstall(commands::uninstall::UninstallArgs),

    /// Ask the daemon to spawn an installed MCP server.
    ///
    /// Refuses if any declared permission is ungranted; in that case
    /// the error message lists the exact `chum permit` calls needed.
    ///
    /// EXAMPLES:
    ///   chum start everything --root /tmp/demo
    ///   chum start filesystem --version 0.1.0 --root /tmp/demo
    Start(commands::start::StartArgs),

    /// Ask the daemon to terminate a running MCP server.
    ///
    /// EXAMPLES:
    ///   chum stop everything --root /tmp/demo
    ///   chum stop everything --grace 2 --root /tmp/demo    # 2s SIGTERM grace
    Stop(commands::stop::StopArgs),

    /// Ask the daemon to stop and re-spawn a running MCP server.
    ///
    /// EXAMPLES:
    ///   chum restart everything --root /tmp/demo
    ///   chum restart everything --json --root /tmp/demo
    Restart(commands::restart::RestartArgs),

    /// Print the daemon-reported status of an installed MCP server.
    ///
    /// Status values: starting | running | restarting | stopped | failed.
    ///
    /// EXAMPLES:
    ///   chum status everything --root /tmp/demo
    ///   chum status everything --json --root /tmp/demo
    Status(commands::status_process::StatusProcessArgs),

    /// Tail recent log lines for an installed MCP server.
    ///
    /// Reads from <install_dir>/logs/{stdout,stderr}.log. v0.1 returns
    /// the last N lines (default 100, max 10_000); --follow / streaming
    /// lands in v0.2.
    ///
    /// EXAMPLES:
    ///   chum logs everything --root /tmp/demo
    ///   chum logs everything --stdout --lines 200 --root /tmp/demo
    ///   chum logs everything --stderr --json --root /tmp/demo
    Logs(commands::logs::LogsArgs),

    /// Grant capability permissions to an installed package.
    ///
    /// Grant string format: <kind>=<value>. Kinds: filesystem.read,
    /// filesystem.write, network.outbound, env.read, subprocess.exec.
    /// Multiple --grant flags accumulate.
    ///
    /// EXAMPLES:
    ///   chum permit brave-search --grant network.outbound=api.search.brave.com --root /tmp/demo
    ///   chum permit brave-search \
    ///       --grant network.outbound=api.search.brave.com \
    ///       --grant env.read=BRAVE_API_KEY \
    ///       --root /tmp/demo
    Permit(commands::permit::PermitArgs),

    /// Revoke a previously-granted permission.
    ///
    /// EXAMPLES:
    ///   chum revoke brave-search --grant env.read=BRAVE_API_KEY --root /tmp/demo
    Revoke(commands::revoke::RevokeArgs),

    /// Show declared vs granted vs missing permissions.
    ///
    /// Three-section diff per package. `missing` is what's blocking
    /// `chum start`.
    ///
    /// EXAMPLES:
    ///   chum permissions brave-search --root /tmp/demo
    ///   chum permissions brave-search --json --root /tmp/demo
    Permissions(commands::permissions::PermissionsArgs),

    /// Manage a package's env vars (set/unset/list).
    ///
    /// Values are written verbatim to <install_dir>/chum-manifest.toml.
    /// The daemon re-reads on next `chum start` / `chum restart`.
    /// `list` prints keys only — values stay opaque.
    ///
    /// EXAMPLES:
    ///   chum env list brave-search --root /tmp/demo
    ///   chum env set brave-search BRAVE_API_KEY=abc123 --root /tmp/demo
    ///   chum env unset brave-search BRAVE_API_KEY --root /tmp/demo
    Env {
        #[command(subcommand)]
        sub: commands::env::EnvSub,
    },

    /// Search first-party + installed packages by name or description.
    ///
    /// On name collision, the installed copy wins (its version and
    /// description show). Default --manifests-dir is `./manifests/`.
    ///
    /// EXAMPLES:
    ///   chum search --root /tmp/demo
    ///   chum search filesystem --root /tmp/demo
    ///   chum search --installed-only --json --root /tmp/demo
    Search(commands::search::SearchArgs),

    /// Diagnostic + control operations against the chumd daemon itself.
    ///
    /// EXAMPLES:
    ///   chum daemon ping
    ///   chum daemon status --json
    ///   chum daemon install-service          # writes ~/Library/LaunchAgents plist + launchctl load
    ///   chum daemon service-status
    ///   chum daemon uninstall-service
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
        Command::Search(args) => {
            let json = args.json;
            (commands::search::run(args).await, json)
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
