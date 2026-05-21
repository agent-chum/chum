# CHUM

> **Homebrew + 1Password for AI agents.**

A local-first MCP package manager and capability broker for AI agents running on Apple Silicon.

## Why

Every agent stack today — Claude Code, OpenClaw, ElizaOS, Cursor — juggles dozens of Model Context Protocol (MCP) servers via hand-edited JSON, scattered `.env` files, and zero trust verification. Cloud-first competitors exist. None of them solve it for **local-first Apple Silicon with crypto-native trust primitives.**

CHUM treats MCP servers and agent skills like packages — installable, sandboxed, updatable, permissioned, observable. The OSS daemon ships first.

## 60-second pitch

```sh
# Coming with v0.1 (~90 days out)
brew install agent-chum/chum/chum

chum install filesystem
chum install brave-search
chum env brave-search BRAVE_API_KEY=...
chum start brave-search

chum list             # health, ports, uptime per server
chum logs brave-search
```

One CLI, one launchd-managed daemon, one local SQLite registry. Every MCP server your agents touch sits behind a capability broker that mediates secrets and permissions.

## Quick start (v0.0.x developer build)

`install`, `list`, and `uninstall` are wired end-to-end against the local registry before the daemon ships. The full lifecycle works today:

```sh
# One-time: create the directory the runnable example points at.
./scripts/setup-test-fixture.sh

# Install. Defaults to $CHUM_HOME, then $XDG_DATA_HOME/chum, then $HOME/.chum;
# pass --root to override for this invocation.
cargo run --bin chum -- install \
    crates/chum-cli/tests/fixtures/chum-local-runnable.toml \
    --root /tmp/chum-demo

# List installed packages. Optional name-prefix filter; --json for scripts.
cargo run --bin chum -- list --root /tmp/chum-demo
cargo run --bin chum -- list chum- --root /tmp/chum-demo --json

# Uninstall. Positional or --version both work; --force skips the y/N prompt
# (also skipped automatically when stdin is not a tty or --json is set).
cargo run --bin chum -- uninstall chum-local-runnable --root /tmp/chum-demo --force

# Dry-run an install to confirm parse + validate without writing:
cargo run --bin chum -- install <path-to-manifest.toml> --dry-run

# Machine-readable JSON for scripting (every command supports it):
cargo run --bin chum -- install <path-to-manifest.toml> --json
cargo run --bin chum -- uninstall foo --keep-files --json   # registry-only delete
```

All three commands compose the same three lower-level crates: `chum-core` parses + validates the manifest, `chum-install` does the filesystem work (symlink for local, fetch + checksum + extract for binary, subprocess for npm), and `chum-registry` persists or reads the row. The daemon will own this composition once it ships in v0.1 — see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the boundary.

### Talking to the daemon

The `chumd` background daemon binary is now wired up with a Unix-socket IPC surface. Start it manually for now (launchd integration ships in a later session):

```sh
# Build + start chumd against an explicit root (defaults to chum_home()).
cargo run --bin chumd -- --root /tmp/chum-demo &

# Two diagnostic verbs are exposed today: ping and status.
cargo run --bin chum -- daemon ping --root /tmp/chum-demo
#  → chumd ok (uptime 3s, 0 installed)

cargo run --bin chum -- daemon status --root /tmp/chum-demo
#  → chumd status
#      pid:              83961
#      started_at:       2026-05-21T13:30:00+00:00
#      installed_count:  0
#      running_count:    0

# --json on either subcommand returns the standard ok-envelope for scripts.
cargo run --bin chum -- daemon ping --root /tmp/chum-demo --json

# --socket-path overrides <root>/daemon.sock on both binaries; useful for
# running multiple chumd instances side-by-side during development.
cargo run --bin chumd -- --socket-path /tmp/alt.sock &
cargo run --bin chum -- daemon ping --socket-path /tmp/alt.sock
```

`chumd` shuts down cleanly on SIGTERM / SIGINT, removes its socket file on exit, and refuses to start over a live socket (returning `another chumd appears to be running`). Stale socket files left by SIGKILL'd previous runs are auto-recovered. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the wire-protocol spec and the stable error codes.

### Process lifecycle

With chumd running you can drive the full install → start → status → stop loop:

```sh
# Install (writes <install_dir>/chum-manifest.toml + logs/ alongside the package).
cargo run --bin chum -- install <manifest.toml> --root /tmp/chum-demo

# Spawn the installed process. The daemon re-parses chum-manifest.toml
# from install_dir and hands it to its in-process supervisor.
cargo run --bin chum -- start <name> --root /tmp/chum-demo

# Daemon-reported status (running | starting | restarting | stopped | failed).
cargo run --bin chum -- status <name> --root /tmp/chum-demo

# Restart in place; restart_count climbs across user-driven restarts.
cargo run --bin chum -- restart <name> --root /tmp/chum-demo

# Stop with the default 5-second SIGTERM grace before SIGKILL.
cargo run --bin chum -- stop <name> --root /tmp/chum-demo

# --grace overrides the SIGTERM window; --json is supported everywhere.
cargo run --bin chum -- stop <name> --grace 2 --json --root /tmp/chum-demo
```

When more than one version of `<name>` is installed, lifecycle subcommands require `--version`; otherwise they return `ambiguous_version` listing the installed versions. The same pattern `chum uninstall` uses.

Per-process stdout / stderr land in `<install_dir>/logs/{stdout,stderr}.log`. `chum logs` lands in a later session — today the files are written but the cli doesn't tail them; cat / tail / less work fine.

## Status

`v0.0.1` — repository scaffold only. v0.1 (CLI + daemon + 10–15 first-party manifests) is targeted for 90 days out. See [`ROADMAP.md`](ROADMAP.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## What CHUM is not

- **Not an agent framework.** CHUM is the control plane around them, not one of them.
- **Not cloud-first.** Local-first is the entire wedge.
- **Not for Windows.** macOS Apple Silicon only. Linux lands in v0.7.

## License

MIT. © 2026 Karoshi. See [`LICENSE`](LICENSE).
