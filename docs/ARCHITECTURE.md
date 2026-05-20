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
- **chum-registry** — local manifest store. SQLite-backed. Caches installed manifests, version pins, dependency graph.
- **chum-core** — shared crate: manifest parsing, schema, signing primitives, common types.
- **chum-ui** *(deferred to v0.4)* — local web UI for monitoring + approval inbox.
- **chum-chain** *(deferred to v0.5)* — on-chain registry contracts.

## Invariants

These are enforced as coding rules in [`CLAUDE.md`](../CLAUDE.md) and reviewed at every PR:

- **chum-core does no I/O.** Pure types, schemas, and parsing only.
- **chum-cli never bypasses chum-daemon.** It is a thin protocol client over the daemon.
- **chum-daemon owns process supervision and state.** All `start` / `stop` / `restart` flows go through it.
- **chum-broker gates all agent ↔ MCP server access.** No direct passthrough; every capability use is mediated.
- **chum-registry is read-write SQLite.** It never mixes concerns with chum-broker.

## Transport surfaces

- **stdio MCP servers** — spawned and supervised as child processes of `chum-daemon`. The broker sits in front of stdin/stdout.
- **HTTP/SSE MCP servers** — bound to localhost ports managed by `chum-daemon`. The broker proxies requests with per-tool authorization.

## Process supervision

`chum-daemon` registers itself with **launchd** at install time. launchd handles auto-start on user login, crash restart with backoff, and clean shutdown on logout. We do not build a custom supervisor.
