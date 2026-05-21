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

The repo ships 10 first-party MCP server manifests under `manifests/`. Pick one, edit any path/key placeholders, install it.

```sh
# Step 1 — edit `manifests/chum-filesystem.toml` so `runtime.args[2]`
# points at a real directory on your machine. The default is
# `/Users/CHANGE_ME/Documents` — an unedited install will fail loudly
# rather than silently bind to that placeholder.
$EDITOR manifests/chum-filesystem.toml

# Step 2 — install. Defaults to $CHUM_HOME, then $XDG_DATA_HOME/chum,
# then $HOME/.chum; pass --root to override for this invocation.
cargo run --bin chum -- install manifests/chum-filesystem.toml --root /tmp/chum-demo

# Step 3 — start the daemon (chumd) and the server.
cargo run --bin chumd -- --root /tmp/chum-demo &
cargo run --bin chum -- start filesystem --root /tmp/chum-demo

# Step 4 — verify everything is running.
cargo run --bin chum -- list --root /tmp/chum-demo
cargo run --bin chum -- status filesystem --root /tmp/chum-demo
cargo run --bin chum -- daemon ping --root /tmp/chum-demo

# Step 5 — stop + uninstall when you're done.
cargo run --bin chum -- stop filesystem --root /tmp/chum-demo
cargo run --bin chum -- uninstall filesystem --root /tmp/chum-demo --force

# Machine-readable JSON for scripting (every command supports it):
cargo run --bin chum -- list --root /tmp/chum-demo --json
cargo run --bin chum -- status filesystem --root /tmp/chum-demo --json
```

Other manifests under `manifests/` (`brave-search`, `slack`, `github`, `postgres`, `puppeteer`, `memory`, `sequential-thinking`, `redis`, `everything`) follow the same pattern. Each manifest's header comment documents what to edit before install (API keys, connection strings, allowed paths). The `chum-everything.toml` reference server needs zero configuration and is the recommended end-to-end smoke target.

#### Local-fixture alternative (no npm required)

If you want to exercise the install / start / stop / status loop without npm or the network, the repo also ships a self-contained Source::Local fixture:

```sh
./scripts/setup-test-fixture.sh
cargo run --bin chum -- install crates/chum-cli/tests/fixtures/chum-local-runnable.toml --root /tmp/chum-demo-local
```

The cli composes three lower-level crates: `chum-core` parses + validates the manifest, `chum-install` does the filesystem work (symlink for local, fetch + checksum + extract for binary, subprocess for npm), and `chum-registry` persists the row. The daemon (`chumd`) owns process supervision and IPC. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the boundary.

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

Per-process stdout / stderr land in `<install_dir>/logs/{stdout,stderr}.log`. The cli reads them via `chum logs`:

```sh
# Default: last 100 lines, both streams with section headers.
cargo run --bin chum -- logs <name> --root /tmp/chum-demo

# Stream-specific:
cargo run --bin chum -- logs <name> --stdout --root /tmp/chum-demo
cargo run --bin chum -- logs <name> --stderr --root /tmp/chum-demo

# Last N lines (capped at 10,000 server-side):
cargo run --bin chum -- logs <name> --lines 500 --root /tmp/chum-demo

# JSON envelope for scripts:
cargo run --bin chum -- logs <name> --json --root /tmp/chum-demo
```

`--follow` (live tailing) lands in v0.2 alongside log rotation — today's logs accumulate verbatim across spawns / restarts in the same files.

### Running CHUM as a service (auto-start on login, macOS)

`chum daemon install-service` writes a `~/Library/LaunchAgents/cloud.chum.daemon.plist` and loads it via `launchctl`. The agent runs `chumd --root <your CHUM_HOME>` whenever you log in and restarts it on crash.

```sh
# Install the LaunchAgent (one-time setup).
cargo run --bin chum -- daemon install-service

# Verify it's running.
cargo run --bin chum -- daemon service-status

# Replace an existing install (e.g. after upgrading chumd):
cargo run --bin chum -- daemon install-service --force

# Stop + remove the LaunchAgent.
cargo run --bin chum -- daemon uninstall-service
```

**Caveat — login-required, not boot-time.** macOS LaunchAgents run only when the user is logged in (FileVault-encrypted volumes mount on login, not at boot). This is the same tradeoff every other LaunchAgent in the wild accepts. If you need boot-time auto-start, you need a LaunchDaemon (system-wide, runs as root) — that's a different security model and lands in a later session if there's demand.

**Env vars baked into the plist.** `chum daemon install-service` reads `$PATH` and `$CHUM_HOME` at install-service time and bakes them directly into the plist's `EnvironmentVariables` dict. `launchctl setenv` is unreliable for LaunchAgents and intentionally not used. If your `$PATH` changes (e.g. you switch shells or install new toolchains), re-run `chum daemon install-service --force`.

Logs from the LaunchAgent itself go to `~/Library/Logs/chum-daemon.{stdout,stderr}.log`. Per-package logs (the ones `chum logs` reads) still live under `<install_dir>/logs/` as before.

### Granting permissions (broker — v0.1 bookkeeping)

Manifests declare what capabilities they need (filesystem paths, network hosts, env vars, subprocess execution). The daemon refuses to spawn until you've granted every declared permission. **v0.1 is bookkeeping, not enforcement** — spawned processes are not actually sandboxed yet; that lands in v0.2 with `sandbox-exec` profiles + env scrubbing. v0.1's job is to record every grant as a deliberate user action.

After `chum install`, the cli prints the exact `chum permit` calls you'll need:

```sh
cargo run --bin chum -- install manifests/chum-brave-search.toml --root /tmp/chum-demo
#  → Installed brave-search 0.1.0 at ...
#  →
#  → This manifest declares permissions you'll need to grant before 'chum start':
#  →     chum permit brave-search --grant network.outbound=api.search.brave.com
#  →     chum permit brave-search --grant env.read=BRAVE_API_KEY

# Grant them. Multiple --grant flags can be passed in one invocation.
cargo run --bin chum -- permit brave-search \
    --grant network.outbound=api.search.brave.com \
    --grant env.read=BRAVE_API_KEY \
    --root /tmp/chum-demo

# Inspect the declared / granted / missing diff at any time:
cargo run --bin chum -- permissions brave-search --root /tmp/chum-demo

# Revoke a single grant (one per invocation in v0.1):
cargo run --bin chum -- revoke brave-search \
    --grant env.read=BRAVE_API_KEY \
    --root /tmp/chum-demo
```

`chum start` against an unpermitted package fails with `permission_denied` and prints the exact `chum permit ...` lines the user is missing. Exact-string matching only in v0.1 — wildcards (`*.anthropic.com`) and prefix matching (`/Users/x` covering `/Users/x/Documents`) land in v0.2.

Five permission kinds, locked at v0.1: `filesystem.read`, `filesystem.write`, `network.outbound`, `env.read`, `subprocess.exec`. Every grant string is `<kind>=<value>`. The full design lives in [`docs/BROKER_DESIGN.md`](docs/BROKER_DESIGN.md).

## Status

`v0.0.1` — repository scaffold only. v0.1 (CLI + daemon + 10–15 first-party manifests) is targeted for 90 days out. See [`ROADMAP.md`](ROADMAP.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## What CHUM is not

- **Not an agent framework.** CHUM is the control plane around them, not one of them.
- **Not cloud-first.** Local-first is the entire wedge.
- **Not for Windows.** macOS Apple Silicon only. Linux lands in v0.7.

## License

MIT. © 2026 Karoshi. See [`LICENSE`](LICENSE).
