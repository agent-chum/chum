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

## CLI composition (v0.1 stopgap)

Until the daemon protocol ships, `chum-cli` composes the three lower-level crates directly inside `crates/chum-cli/src/commands/`. Once `chum-daemon` exists, every subcommand sends a request over its protocol surface and the same orchestration moves behind it. The pipeline shape does not change — only the boundary moves.

```
chum install <manifest>                     chum list                       chum uninstall <name> [version]
   │                                         │                                │
   ▼                                         ▼                                ▼
chum-core::parse_and_validate                ─── (no chum-core needed)        ─── (no chum-core needed)
   │                                         │                                │
   ▼                                         │                                │
chum-install::install                        │                                │
   │   returns InstalledArtifact             │                                │
   ▼                                         ▼                                ▼
chum-registry::Registry::insert     chum-registry::list_all / list_by_name    chum-registry::get_by_name_version
   │                                         │                                │
   │                                         │                                ▼
   │                                         │                          fs::remove_dir_all (unless --keep-files)
   │                                         │                                │
   │                                         │                                ▼
   │                                         │                          chum-registry::delete
   ▼                                         ▼                                ▼
print confirmation                  print table or JSON                print confirmation
```

What the cli adds on top of the three crates:

- **Single ErrorRenderer.** `chum_cli::error::UserFacingError` wraps every crate-level error and maps it to a stable `code` string plus a human message. Library types never reach `stderr` directly. Codes are part of the `--json` contract — see `crates/chum-cli/src/error.rs::UserFacingError::code`.
- **Shared `resolve_root` helper.** All three subcommands route `--root` override and `chum_home()` resolution through `commands::resolve_root`, so the `chum_home_unresolved` error path is one piece of code, not three copies.
- **`--dry-run`** (install only). Parse + validate + root resolution; no filesystem or registry I/O. The resolved root is echoed so users can confirm a `--root` override took effect.
- **`--json` envelopes.** Stable contracts for scripting:
  - install ok → `{"status":"ok","installed":{...}}`
  - install dry-run → `{"status":"dry-run","manifest":{...},"root":"...","would_install_at":"..."}`
  - list → `{"status":"ok","packages":[...]}`
  - uninstall ok → `{"status":"ok","uninstalled":{"name":"...","version":"...","keep_files":false}}`
  - uninstall cancelled (tty + "n") → `{"status":"cancelled","name":"...","version":"..."}`
  - any error → `{"status":"error","code":"...","message":"..."}`
- **Duplicate pre-check on install.** Before calling `chum-install`, the cli asks the registry whether `(name, version)` already exists. Defense in depth on top of `UNIQUE(name, version)` in SQL — lets us return `already_installed` (clearer than `registry_duplicate`) and avoid touching the filesystem at all on a re-install.
- **Registry-driven version resolution on uninstall.** If the caller does not supply a version, `list_by_name(name)` decides: 0 rows → `not_installed`, 1 → that one, 2+ → `ambiguous_version` carrying the list of versions so the message can name them. The cli deliberately does **not** implement an implicit "pick the latest" rule — silent guessing on a destructive operation is the wrong default.
- **TTY-aware uninstall confirmation.** `std::io::stdin().is_terminal()` (Rust 1.85+, no `atty` / `is-terminal` crate) gates the y/N prompt. Skip rules: `--force` OR `--json` OR not-a-tty.
- **`list` does not create `state.db`.** A fresh root with no install_artifacts table is treated as an empty list (`No packages installed.`, exit 0). The cli checks `db.is_file()` before calling `Registry::open` so a bare `chum list` on an empty machine doesn't leave a 16-byte SQLite file behind.

`commands/install.rs`, `commands/list.rs`, and `commands/uninstall.rs` each carry a `// TODO(chum-v0.x): route through chum-daemon protocol once it lands.` marker at the top. Future contributors should not extend the direct-composition surface — new subcommands wait for the daemon protocol.

## chum-registry storage (v0.1)

### Schema

The v0.1 schema is a single domain table plus a one-row version marker. `state.db` is created by `Registry::open` on first use; the migration runner advances it to `CURRENT_SCHEMA_VERSION` (currently `1`).

```sql
CREATE TABLE schema_version (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL
);

CREATE TABLE installed_artifacts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    install_dir  TEXT NOT NULL,                                   -- canonical path
    entrypoint   TEXT NOT NULL,                                   -- per SourceKind
    source_kind  TEXT NOT NULL CHECK (source_kind IN ('npm', 'local', 'binary')),
    installed_at TEXT NOT NULL,                                   -- ISO 8601 UTC
    UNIQUE(name, version)
);
```

Notes:

- **`installed_at` is registry-owned.** `chum-install`'s `InstalledArtifact` describes *what landed on disk*; *when it was recorded* is a registry fact. `Registry::insert` stamps `Utc::now()`, keeping `chrono` out of every caller and preventing the install/registry boundary from leaking a timestamp concern.
- **`source_kind` uses a CHECK constraint, not an `ENUM`.** SQLite has no enums; CHECK is the closest equivalent and lets a forward migration extend the allowed set without rewriting the column. A future `SourceKind::Pypi` becomes `ALTER TABLE … CHECK (source_kind IN ('npm', 'local', 'binary', 'pypi'))` in a new migration.
- **`UNIQUE(name, version)`** is the integrity constraint that surfaces as `RegistryError::DuplicateArtifact` on `insert`.

### Migration philosophy

Migrations are an append-only list. Each entry brings the database from schema version `N` to `N + 1`, runs inside its own transaction, and atomically updates `schema_version` as part of that transaction. The rules:

- **Append, never edit.** Once a migration ships, its SQL is frozen. Bugs are fixed by adding a new migration on top.
- **One logical change per migration.** A migration adds a column, creates a table, or rewrites data — not a mix.
- **Version is bumped inside the same transaction.** A partial migration rolls back as a unit; we never end up with the table half-altered and `schema_version` half-updated.
- **The runner refuses to operate on a future-version database.** If `state.db` reports a schema version higher than `CURRENT_SCHEMA_VERSION`, `Registry::open` returns `MigrationFailed` instead of silently corrupting forward-only state. This is the one case where the binary tells the user to upgrade.
- **No `DROP TABLE` of domain tables in v0.x.** Renames and column adds only. v1.0 is where backwards-incompatible schema changes become possible, and then only with an explicit migration policy bump.

### Read/write boundary recap

| Operation | Method | Returns |
|---|---|---|
| Open or create | `Registry::open(path)` | `Result<Registry, RegistryError>` |
| Insert one | `registry.insert(&InstalledArtifact)` | `Result<i64, RegistryError>` |
| Get one | `registry.get_by_name_version(name, version)` | `Result<RegistryArtifact, RegistryError>` |
| List all | `registry.list_all()` | `Result<Vec<RegistryArtifact>, RegistryError>` |
| List by name | `registry.list_by_name(name)` | `Result<Vec<RegistryArtifact>, RegistryError>` |
| Delete one | `registry.delete(name, version)` | `Result<(), RegistryError>` |
| Read schema version | `registry.schema_version()` | `Result<i64, RegistryError>` |

`Registry` is `!Sync` (it wraps a single `rusqlite::Connection`). The daemon owns one instance and serialises access; tests instantiate one per `TempDir`.

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
