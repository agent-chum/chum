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

/// Subcommands exposed by the CLI. v0.1 ships `install` only; more
/// land in subsequent sessions.
#[derive(Subcommand, Debug)]
enum Command {
    /// Install an MCP server from a manifest TOML file.
    Install(commands::install::InstallArgs),
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let (result, json) = match cli.command {
        Command::Install(args) => {
            let json = args.json;
            (commands::install::run(args).await, json)
        }
    };
    if let Err(e) = result {
        error::render(&e, json);
        std::process::exit(1);
    }
}
