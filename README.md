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

## Status

`v0.0.1` — repository scaffold only. v0.1 (CLI + daemon + 10–15 first-party manifests) is targeted for 90 days out. See [`ROADMAP.md`](ROADMAP.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## What CHUM is not

- **Not an agent framework.** CHUM is the control plane around them, not one of them.
- **Not cloud-first.** Local-first is the entire wedge.
- **Not for Windows.** macOS Apple Silicon only. Linux lands in v0.7.

## License

MIT. © 2026 Karoshi. See [`LICENSE`](LICENSE).
