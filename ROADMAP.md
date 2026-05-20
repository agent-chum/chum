# CHUM Roadmap

CHUM ships in versioned slices. Each release is a self-contained step toward a local-first MCP control plane on Apple Silicon. Dates are targets, not commitments.

## v0.1 — Day 90 — *foundation*

CLI + daemon + 10–15 first-party manifests + launchd integration.

**Commands shipped:**

- `chum install <server>` / `chum uninstall <server>`
- `chum start <server>` / `chum stop <server>` / `chum restart <server>`
- `chum list` / `chum status <server>` / `chum logs <server>`
- `chum env <server>` — scoped secrets per server
- `chum permit <server> <capability>` — basic permission grants
- `chum update <server>` / `chum search <query>`

**Infrastructure:**

- Launchd integration — daemon auto-starts on login, restarts on crash.
- Manifest format spec v0.1 (TOML) with validation and dependency resolution.
- First-party manifests for `git`, `filesystem`, `brave-search`, `memory`, `fetch`, `sqlite`, `slack`, `postgres`, `puppeteer`, `sequential-thinking`, plus a showcase package and a handful selected by community demand.

**Distribution:**

- Homebrew tap (`brew install agent-chum/chum/chum`)
- GitHub release binaries (macOS arm64 + x86_64)
- One-line install (`curl chum.dev/install.sh | sh`)

**Explicitly out of v0.1:** process sandboxing, manifest signing, approval inbox, on-chain registry, multi-device sync, Linux support, Windows support.

## v0.2 — Day 150 — *capability broker*

Per-tool grants, scoped secrets, path allowlists, network zones. The permission model graduates from basic grants into a real broker.

## v0.3 — Day 200 — *signed manifests*

Sigstore-compatible signing, verification at install time, key management. Lays the groundwork for the public registry.

## v0.4 — Day 270 — *approval inbox*

Local web UI plus mobile push for human-in-the-loop approval on high-risk actions.

## v0.5 — Day 330 — *public manifest registry*

On-chain trust layer for the public registry. Design lives in a separate document; this roadmap entry exists to flag that the registry surfaces in v0.5 alongside the OSS daemon.

## v0.6 — Day 400 — *local-cloud sync prototype*

Append-only journal + CRDT for multi-device sync of installed servers and grants.

## v0.7 — Day 460 — *Linux support*

Systemd integration. Same CLI and daemon surface; different process supervisor.

## v1.0 — Day 540 — *polish*

Stability, monitoring UI, public registry maturity, documentation pass.

## What this roadmap does not promise

- **Windows support.** Never. The local-first Apple-Silicon wedge is the project; Windows is out of scope forever.
- **Cloud-only features.** Local-first is the entire design.
- **Agent frameworks.** CHUM is the control plane around frameworks, not one of them.
