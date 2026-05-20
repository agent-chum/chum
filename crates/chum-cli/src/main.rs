//! `chum` — the CHUM CLI.
//!
//! Per the architecture invariants in `CLAUDE.md`, this binary is a thin
//! layer over the `chum-daemon` protocol. It never bypasses the daemon to
//! reach MCP servers or the manifest store directly.
//!
//! v0.0.1 scaffold: only `--version` and `--help` are wired up. Real
//! subcommands arrive alongside the daemon protocol.

#![forbid(unsafe_code)]

use clap::Parser;

/// Top-level `chum` CLI invocation.
///
/// Holds no fields yet — subcommands are added once `chum-daemon` exposes a
/// protocol surface to call into.
#[derive(Parser, Debug)]
#[command(
    name = "chum",
    version,
    about = "Local-first MCP package manager and capability broker."
)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    Ok(())
}
