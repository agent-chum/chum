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

The `chum install` pipeline is wired end-to-end against a local fixture before the daemon ships. Try it against a Source::Local manifest:

```sh
# One-time: create the directory the runnable example points at.
./scripts/setup-test-fixture.sh

# Build + run the CLI against the runnable fixture.
cargo run --bin chum -- install crates/chum-cli/tests/fixtures/chum-local-runnable.toml

# Or specify your own root for the install (defaults to $CHUM_HOME, then
# $XDG_DATA_HOME/chum, then $HOME/.chum):
cargo run --bin chum -- install <path-to-manifest.toml> --root /tmp/chum-demo

# Dry-run any manifest to confirm parse + validate without writing:
cargo run --bin chum -- install <path-to-manifest.toml> --dry-run

# Machine-readable JSON for scripting:
cargo run --bin chum -- install <path-to-manifest.toml> --json
```

`chum install` composes the three lower-level crates (`chum-core` parses the manifest, `chum-install` symlinks / fetches / extracts, `chum-registry` persists the row). The daemon will own this composition in v0.1; see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the boundary.

## Status

`v0.0.1` — repository scaffold only. v0.1 (CLI + daemon + 10–15 first-party manifests) is targeted for 90 days out. See [`ROADMAP.md`](ROADMAP.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## What CHUM is not

- **Not an agent framework.** CHUM is the control plane around them, not one of them.
- **Not cloud-first.** Local-first is the entire wedge.
- **Not for Windows.** macOS Apple Silicon only. Linux lands in v0.7.

## License

MIT. © 2026 Karoshi. See [`LICENSE`](LICENSE).
