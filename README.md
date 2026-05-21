# CHUM

> **Homebrew + 1Password for AI agents.** A local-first MCP package manager and capability broker for Apple Silicon.

[![CI](https://github.com/agent-chum/chum/actions/workflows/ci.yml/badge.svg)](https://github.com/agent-chum/chum/actions/workflows/ci.yml)

## What CHUM does

- **Installs and uninstalls** Model Context Protocol servers as packages, from a typed TOML manifest, with checksum-verified binaries and version-pinned npm sources.
- **Supervises** every MCP server through a launchd-managed daemon (`chumd`) — auto-start on login, restart on crash, per-package stdout/stderr log files.
- **Gates spawn-time capabilities** via the v0.1 broker: every manifest declares the permissions it needs (filesystem paths, network hosts, env vars, subprocess execution); users grant them explicitly with `chum permit` before `chum start` will run anything.
- **Stays local-first.** No cloud calls, no telemetry, no opaque daemons. State lives in a single SQLite file at `<chum_home>/state.db`; everything else is your filesystem.

## Quick start (copy-paste, no credentials needed)

The repo ships 10 first-party MCP server manifests under `manifests/`. `chum-everything.toml` is the reference / demo server — zero env vars, zero path edits, runs end-to-end out of the box.

```sh
# Build the workspace once.
cargo build --release --workspace

# Install the everything-server demo manifest.
cargo run --bin chum -- install manifests/chum-everything.toml --root /tmp/chum-demo

# Start chumd in the background.
cargo run --bin chumd -- --root /tmp/chum-demo &

# Start the MCP server. The broker waves it through — chum-everything
# declares zero permissions by design.
cargo run --bin chum -- start everything --root /tmp/chum-demo

# Verify it's running.
cargo run --bin chum -- list --root /tmp/chum-demo
cargo run --bin chum -- status everything --root /tmp/chum-demo
cargo run --bin chum -- daemon ping --root /tmp/chum-demo

# Read recent logs (tail of stdout + stderr).
cargo run --bin chum -- logs everything --root /tmp/chum-demo

# Stop + uninstall.
cargo run --bin chum -- stop everything --root /tmp/chum-demo
cargo run --bin chum -- uninstall everything --root /tmp/chum-demo --force

# Teardown.
kill %1     # stops chumd
```

For a manifest with real capability requirements (e.g. `chum-brave-search.toml`), the lifecycle is the same but you'll need a `chum permit` step between install and start. `chum install` prints the exact `chum permit` lines you need — copy them verbatim.

## What works in v0.1

- **CLI commands:** `install`, `uninstall`, `list`, `search`, `start`, `stop`, `restart`, `status`, `logs`, `env` (`set`/`unset`/`list`), `permit`, `revoke`, `permissions`, `daemon ping`, `daemon status`, `daemon install-service`, `daemon uninstall-service`, `daemon service-status`.
- **Stable `--json` envelopes** on every command for scripting. Error envelopes carry a stable `code` plus an optional `hint` field.
- **Process supervision** with a configurable restart policy (`always` / `on-failure` / `never`) and exponential backoff (1s → 16s capped).
- **launchd integration** — `chum daemon install-service` writes the LaunchAgent and loads it. chumd auto-starts on user login and restarts on crash.
- **Capability bookkeeping** — broker validates that manifest-declared permissions are granted at spawn time. Five categories: `filesystem.read`, `filesystem.write`, `network.outbound`, `env.read`, `subprocess.exec`. **v0.1 is bookkeeping only — spawned processes are not sandboxed yet.** Real enforcement (sandbox-exec, env scrubbing) ships in v0.2 against the same data model.
- **10 first-party manifests** under `manifests/` for the most-used Anthropic MCP servers: filesystem, brave-search, sequential-thinking, memory, puppeteer, postgres, slack, github, redis, everything.
- **Recovery hints** on every error class. `error: <what>` is followed by `hint: <how to fix>` for the variants that benefit. Doubles as the `"hint"` field in `--json` error envelopes.

## What's coming in v0.2

Real broker enforcement (sandbox-exec profiles + env scrubbing + network filtering), wildcard / prefix matching on grants, log rotation, `chum update` for version-bump flows, Pypi-source manifests (`mcp-server-git`, `-fetch`, `-sqlite`), and `chum logs --follow`. The full backlog is grep-able:

```sh
grep -rn 'TODO(chum-v0.2)' crates/ docs/
```

## Architecture

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) is the canonical reference for how the six crates compose: the install/registry/daemon boundary, the chumd IPC protocol (JSON Lines over a Unix socket), the supervisor + broker pipeline, the launchd plist schema, and the v0.1 stopgaps that move behind the daemon in later sessions.

[`docs/BROKER_DESIGN.md`](docs/BROKER_DESIGN.md) details the broker's v0.1 surface and the v0.2 enforcement seam.

## Scope and non-goals

- **macOS only in v0.1.** Linux (systemd) ships in v0.7 per [`ROADMAP.md`](ROADMAP.md). Windows is out of scope forever.
- **OSS daemon first.** The Base-chain token registry layer is a v0.5+ concern, not part of this codebase.
- **Not an agent framework.** CHUM is the control plane around frameworks like Claude Code, OpenClaw, ElizaOS — not a competing runtime.

## License

MIT. © 2026 Karoshi. See [`LICENSE`](LICENSE).
