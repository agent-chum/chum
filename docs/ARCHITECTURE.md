# CHUM Architecture

## Diagram

```
                              ┌─────────────────────┐
                              │   chum-cli          │
                              │   (user-facing)     │
                              └──────────┬──────────┘
                                         │
                              ┌──────────▼──────────┐
                              │   chum-daemon       │
                              │   (launchd-managed) │
                              └──────────┬──────────┘
                                         │
        ┌────────────────────┬───────────┼───────────┬────────────────────┐
        │                    │           │           │                    │
┌───────▼────────┐  ┌────────▼────────┐  │  ┌────────▼─────────┐  ┌──────▼──────┐
│  chum-registry │  │  chum-broker    │  │  │  Manifest store  │  │ Process     │
│  (manifests)   │  │  (permissions   │  │  │  (local SQLite)  │  │ supervisor  │
│                │  │   + secrets)    │  │  └──────────────────┘  │ (launchd)   │
└────────┬───────┘  └────────┬────────┘  │                         └─────────────┘
         │                   │           │
         │            ┌──────▼───────────▼──────┐
         │            │   MCP servers running   │
         │            │   (stdio + HTTP/SSE)    │
         │            └─────────────────────────┘
         │
   ┌─────▼──────────────────────────────────┐
   │  (v0.5+) Public manifest registry      │
   │  Trust + governance layer — design TBD │
   └────────────────────────────────────────┘
```

## Components

- **chum-cli** — user-facing CLI. Single static Rust binary. Distributed via Homebrew tap + GitHub releases.
- **chum-daemon** — long-running supervisor process. Managed by launchd. Handles MCP server lifecycle, health checks, restart policies, log aggregation.
- **chum-broker** — permission and secrets broker. Mediates between agents/clients and MCP servers. Per-tool grants, scoped secrets, path allowlists.
- **chum-install** — install-time I/O. Fetches binaries, runs `npm install` subprocesses, symlinks local sources, verifies SHA-256 checksums, extracts archives. Returns an `InstalledArtifact` describing where things landed on disk; **does not persist anything itself**.
- **chum-registry** — local install-record store. SQLite-backed. Records `InstalledArtifact` rows, version pins, dependency graph. Read-write but does not act on the filesystem beyond its own database file.
- **chum-core** — shared crate: manifest parsing, schema, signing primitives, common types.
- **chum-ui** *(deferred to v0.4)* — local web UI for monitoring + approval inbox.
- **chum-chain** *(deferred to v0.5)* — on-chain registry contracts.

## The install / registry boundary

`chum-install` and `chum-registry` are split deliberately so that **acting on the filesystem** and **recording what was acted on** never co-mingle. Each crate's territory on disk is disjoint:

| Path | Owner | Operations |
|---|---|---|
| `<root>/packages/<name>/<version>/` | chum-install | create, populate (npm `node_modules/`, local `local-src/`, binary `bin/`) |
| `<root>/bin/` | chum-install | symlinks placed by binary installs |
| `<root>/cache/downloads/` | chum-install | in-flight fetch buffers |
| `<root>/state.db` | chum-registry | exclusive SQLite read/write |

The install pipeline:

```
chum-cli install <name>
   │
   ▼
chum-daemon orchestrates
   │
   ▼
chum-install ACTS — fetch + verify + extract + symlink
   │   returns InstalledArtifact { install_dir, entrypoint, source_kind, ... }
   ▼
chum-registry PERSISTS — INSERT into state.db
```

Later, at start time:

```
chum-daemon reads state.db
   │
   ▼
chum-registry returns the InstalledArtifact
   │
   ▼
chum-daemon spawns the server at artifact.entrypoint
```

`chum-install` never writes to `state.db`. `chum-registry` never writes to `packages/` or `bin/`. The daemon orchestrates the handoff.

## Invariants

These are enforced as coding rules in [`CLAUDE.md`](../CLAUDE.md) and reviewed at every PR:

- **chum-core does no I/O.** Pure types, schemas, and parsing only.
- **chum-cli never bypasses chum-daemon.** It is a thin protocol client over the daemon.
- **chum-daemon owns process supervision and state.** All `start` / `stop` / `restart` flows go through it.
- **chum-broker gates all agent ↔ MCP server access.** No direct passthrough; every capability use is mediated.
- **chum-install acts but does not persist.** It writes files and symlinks under `packages/` / `bin/` and returns an `InstalledArtifact`. It does not touch `state.db`.
- **chum-registry persists but does not act.** It reads and writes `state.db`. It does not modify `packages/` or `bin/`.
- **chum-registry is read-write SQLite.** It never mixes concerns with chum-broker.

## Transport surfaces

- **stdio MCP servers** — spawned and supervised as child processes of `chum-daemon`. The broker sits in front of stdin/stdout.
- **HTTP/SSE MCP servers** — bound to localhost ports managed by `chum-daemon`. The broker proxies requests with per-tool authorization.

## Process supervision

`chum-daemon` registers itself with **launchd** at install time. launchd handles auto-start on user login, crash restart with backoff, and clean shutdown on logout. We do not build a custom supervisor.
