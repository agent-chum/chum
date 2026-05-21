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

## chumd IPC protocol (v0.1)

The `chumd` background daemon exposes a tiny diagnostic surface over a Unix domain socket at `<chum_home>/daemon.sock` (chmod `0600`). The wire format is **JSON Lines** — one request per connection, terminated by `\n`, then one response, then the daemon closes. Pipelining and streaming verbs are deferred to a later session.

### Request

```json
{"protocol_version":1,"verb":"ping","args":null}
```

| Field | Type | Notes |
|---|---|---|
| `protocol_version` | unsigned integer | Must equal `1` for this daemon build. Mismatch → `unsupported_protocol_version`. |
| `verb` | string | Routing key. v0.1 verbs: `ping`, `status`, `list_processes`. |
| `args` | JSON | Optional verb-specific payload. Always `null` for the v0.1 verbs. |

### Response — ok

```json
{"protocol_version":1,"status":"ok","data":{...}}
```

### Response — error

```json
{"protocol_version":1,"status":"error","code":"unknown_verb","message":"..."}
```

`code` is one of the stable strings in `crates/chum-daemon/src/ipc/mod.rs::codes`. Scripts pattern-match on these:

| Code | When it fires |
|---|---|
| `unsupported_protocol_version` | Request's `protocol_version` differs from the daemon's `PROTOCOL_VERSION`. |
| `unknown_verb` | Verb string is not in the dispatch table. |
| `invalid_request` | Request body is empty, not JSON, or fails the `Request` schema. |
| `request_too_large` | Request line exceeded the daemon's hard 64 KiB cap. |
| `request_timeout` | Client opened a connection and sent nothing within the daemon's 5s read window. |
| `internal` | Unrecoverable server-side fault. Bug in chumd; should not happen on the v0.1 verb set. |

### Verbs (v0.1)

| Verb | `data` shape |
|---|---|
| `ping` | `{ "daemon_version": "0.1.0", "uptime_secs": N, "installed_count": N }` |
| `status` | `{ "pid": N, "started_at": "<rfc3339>", "installed_count": N, "running_count": N }` |
| `list_processes` | `{ "processes": [ { "name", "version", "status" }, … ] }` — always empty in v0.1; locked shape for Session B. |

`installed_count` is a snapshot taken once at daemon startup (`chum-registry::list_all()` on the boot root). It is *not* refreshed for the daemon's lifetime in v0.1 — Session B introduces a refresh path triggered by install / uninstall. `running_count` is `Supervisor::list().len()`, which is always 0 in v0.1 because no verb spawns into the supervisor yet.

### Graceful shutdown

`chumd` installs handlers for `SIGTERM` and `SIGINT`. On either, the accept loop stops, in-flight handlers drain up to a 5s ceiling, the socket file is removed, and the process exits `0`. A `SIGKILL`'d chumd leaves a stale socket file behind; the next `chumd` start `connect()`-tests the existing path and either fails fast (live chumd) or removes the stale file (no connection). There is no pidfile — a SIGKILL'd run can't update one, so it'd lie.

### Client

`chum-daemon::DaemonClient` is the canonical client. It exposes a low-level `request(&Request)` for scripts driving the protocol directly plus typed `ping() / status() / list_processes()` methods that decode `data` into typed structs. The cli's `chum daemon ping / status` subcommands wrap it.

### Lifecycle verbs (process supervision)

Past the v0.1 diagnostic verbs, chumd exposes four lifecycle verbs that drive its in-process `Supervisor`. Wire shape:

| Verb | `args` | Ok `data` | Stable error codes |
|---|---|---|---|
| `spawn` | `{name, version}` | `{pid, started_at}` | `process_not_installed`, `manifest_missing_in_install_dir`, `manifest_invalid`, `process_already_running`, `spawn_failed` |
| `terminate` | `{name, version, grace_secs?}` | `{stopped: true}` | `process_not_running`, `kill_failed`, `monitor_wedged` |
| `restart` | `{name, version}` | `{pid, started_at, restart_count}` | `process_not_running`, `spawn_failed` |
| `process_status` | `{name, version}` | `{name, version, status, pid?, restart_count, exit_code?}` | `process_not_installed` |

`list_processes` is extended in v0.1 to return the same per-process fields (`name`, `version`, `status`, `pid?`, `restart_count`, `exit_code?`). `restart_count` here is the **user-driven** count maintained by chumd's `DaemonState` — distinct from the supervisor's internal `restart_count` which counts policy-driven respawns. Spawn resets the count to 0; restart increments; terminate removes the entry.

#### Spawn flow

```
chum start <name> [--version V]
        │
        │ resolve_lifecycle_target  (cli — registry lookup + ambiguity check)
        │
        ▼
chumd IPC spawn { name, version }
        │
        ▼
DaemonState::supervisor (chum_daemon::Supervisor)
        │
        │ 1. registry.get_by_name_version(name, version)  ── RegistryArtifact
        │ 2. fs::read_to_string(<install_dir>/chum-manifest.toml)
        │ 3. chum_core::parse_and_validate
        │ 4. Supervisor::spawn(InstalledArtifact, Manifest)
        │ 5. monitor task owns Child, redirects stdout/stderr to
        │    <install_dir>/logs/{stdout,stderr}.log (append)
        │
        ▼
SpawnResponse { pid, started_at }
```

The manifest re-parse on every spawn is by design: `chum-install` writes `<install_dir>/chum-manifest.toml` at install time exactly so the supervisor's runtime config (command, args, env, lifecycle policy) is recoverable without keeping in-memory state across daemon restarts. The registry stays narrow — it persists *what is installed*, not *how to run it*.

#### Logs

Child stdout / stderr are redirected to `<install_dir>/logs/{stdout,stderr}.log` opened with `OpenOptions::create(true).append(true)`. Both internal supervisor restarts (policy-driven) and user-driven restarts re-use the same files, so log files accumulate across the package's lifetime.

The `tail_logs` IPC verb (and `chum logs <name>` cli wrapping it) read the last N lines on demand. Wire shape:

| Verb | `args` | Ok `data` | Stable error codes |
|---|---|---|---|
| `tail_logs` | `{name, version, stream: "stdout"\|"stderr"\|"both", lines: N}` | `{stream, content}` | `process_not_installed`, `logs_unavailable`, `logs_invalid_stream`, `logs_lines_too_large` |

Defaults: `stream = "both"`, `lines = 100`. The daemon enforces `1 <= lines <= 10_000`. For `stream == "both"`, the content is `=== stdout.log (last N lines) ===\n<stdout>\n=== stderr.log (last N lines) ===\n<stderr>` — concat with section headers rather than timestamp interleaving (which would require parsing log content, deferred to v0.2).

`logs_unavailable` fires when the package has never been spawned (no log files yet) or the `logs/` directory was hand-removed. The cli renders it with a hint to `chum start <name>` once.

Log rotation and `--follow` / streaming land in v0.2 — both need a long-lived IPC channel and a file-size watermark policy that don't fit the v0.1 surface. TODO markers in `ipc/server.rs::read_tail` point at the chunked reverse-read follow-up for huge log files.

#### Migration: pre-manifest-copy installs

Installs created before `feat(install): copy manifest to install_dir + create logs dir` shipped don't have `chum-manifest.toml` in their install_dir. Calling `chum start` against such a row surfaces `manifest_missing_in_install_dir` from the daemon, which the cli renders as a re-install hint. No automatic migration is attempted — re-installing the package is the supported recovery path.

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

## launchd integration (v0.1, macOS)

`chum daemon install-service` writes a LaunchAgent plist at `~/Library/LaunchAgents/cloud.chum.daemon.plist` and runs `launchctl load` against it. The agent owns the daemon's lifecycle from then on — auto-start on user login, restart on crash, clean shutdown on logout.

```
chum daemon install-service
        │
        ▼
ServiceConfig::resolve
   - chumd_path  = current_exe sibling, override via --chumd-path
   - chum_home   = chum_home(), override via --root
   - plist_path  = ~/Library/LaunchAgents/cloud.chum.daemon.plist
   - log paths   = ~/Library/Logs/chum-daemon.{stdout,stderr}.log
   - PATH        = current $PATH baked verbatim into the plist
        │
        ▼
render_plist (format! template, xml_escape on paths)
        │
        ▼
write to plist_path (chmod 644)
        │
        ▼
launchctl load <plist_path>
```

### Plist schema (locked at v0.1)

```xml
<plist version="1.0"><dict>
    <key>Label</key>                <string>cloud.chum.daemon</string>
    <key>ProgramArguments</key>     <array>
                                       <string>{chumd_path}</string>
                                       <string>--root</string>
                                       <string>{chum_home}</string>
                                    </array>
    <key>RunAtLoad</key>            <true/>
    <key>KeepAlive</key>            <dict>
                                       <key>SuccessfulExit</key>
                                       <false/>
                                    </dict>
    <key>EnvironmentVariables</key> <dict>
                                       <key>PATH</key>
                                       <string>{path_env}</string>
                                       <key>CHUM_HOME</key>
                                       <string>{chum_home}</string>
                                    </dict>
    <key>StandardOutPath</key>      <string>{log_dir}/chum-daemon.stdout.log</string>
    <key>StandardErrorPath</key>    <string>{log_dir}/chum-daemon.stderr.log</string>
    <key>WorkingDirectory</key>     <string>{chum_home}</string>
</dict></plist>
```

`KeepAlive.SuccessfulExit = false` means: restart the daemon on crash (non-zero exit), but don't restart it on a clean exit. Clean exits are the SIGTERM / SIGINT-driven graceful shutdown path; restarting then would loop chumd forever against a user-issued stop.

### Env vars baked, not `launchctl setenv`

`launchctl setenv` is documented but does not propagate reliably for LaunchAgents in practice — affected sessions, stale values across reboots, no clean way to scope per-Label. CHUM bakes `PATH` + `CHUM_HOME` directly into the plist's `EnvironmentVariables` dict at `install-service` time. If the user's `$PATH` changes (new toolchain, new shell), they re-run `chum daemon install-service --force` to refresh the plist. The decision is documented in `commands/daemon_service.rs::ServiceConfig::resolve`.

### Login-only, not boot-time

LaunchAgents run only when the user is logged in (FileVault volumes mount on login, not at boot — the user's home directory isn't readable to root before login). Boot-time auto-start needs a LaunchDaemon (system-wide, runs as root), which has a different security model and is deferred to a later session.

### Zombie / re-install path

`install-service --force` calls `launchctl unload` (best-effort, ignored failure) before overwriting the plist + re-loading. This handles "the user upgraded chumd but the old LaunchAgent is still loaded" — a clean re-install in one command. `uninstall-service` is idempotent: unload + remove, both no-op if the plist is already gone.

### Status via `launchctl list`

`chum daemon service-status` runs `launchctl list cloud.chum.daemon` and line-parses the OpenStep-format output for `"PID"` and `"LastExitStatus"`. No `plist` crate dependency — the output isn't XML, it's NeXTSTEP-style, and a 20-line line scanner handles the two fields the cli surfaces. Implementation in `commands/daemon_service.rs::parse_int_field`.

## chum-broker (v0.1 bookkeeping)

`chum-broker` is the capability-verification primitive that gates `Supervisor::spawn`. v0.1 is **bookkeeping only**: the manifest declares what permissions the MCP server needs, the user grants them via `chum permit`, the daemon refuses to spawn if any required permission is ungranted. **No actual sandboxing in v0.1.** v0.2 adds sandbox-exec profile generation + env scrubbing + network filtering using the same data — see [`docs/BROKER_DESIGN.md`](BROKER_DESIGN.md) for the full design.

### Permission categories (v0.1)

Five wire codes, locked at v0.1:

| Code | Value shape | Example |
|---|---|---|
| `filesystem.read` | absolute path | `/Users/x/Documents` |
| `filesystem.write` | absolute path | `/tmp/chum-workspace` |
| `network.outbound` | host (no scheme, no port) | `api.search.brave.com` |
| `env.read` | env var name | `BRAVE_API_KEY` |
| `subprocess.exec` | program name or absolute path | `git`, `/usr/bin/curl` |

**Granularity: exact-string match.** A grant for `/Users/x` does NOT cover `/Users/x/Documents`; a grant for `anthropic.com` does NOT cover `api.anthropic.com`. v0.2's enforcement layer adds wildcard / prefix matching when usability needs it.

### Pipeline

```
chum start <name>
        │
        ▼
verb_spawn (chum-daemon)
   │
   │  1. registry.get_by_name_version  ──► InstalledArtifact { id, ... }
   │  2. fs::read_to_string(install_dir/chum-manifest.toml)
   │  3. chum_core::parse_and_validate ──► Manifest { permissions, ... }
   │  4. registry.list_grants(artifact.id) ──► Vec<Grant>
   │  5. chum_broker::validate(&manifest.permissions, &grants)
   │           │
   │           ▼ Allow                          ▼ Deny { unmet }
   ▼           │                                 │
6. Supervisor::spawn                Response::error("permission_denied",
                                       "<name> <version> requires grants
                                       not yet given: <list>. Run: chum
                                       permit <name> --grant <kind>=<value>")
```

### Registry storage

Migration 2 adds the `permission_grants` table:

```sql
CREATE TABLE permission_grants (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    artifact_id INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
                    'filesystem.read', 'filesystem.write',
                    'network.outbound', 'env.read', 'subprocess.exec'
                )),
    value       TEXT NOT NULL,
    granted_at  TEXT NOT NULL,
    UNIQUE(artifact_id, kind, value),
    FOREIGN KEY (artifact_id) REFERENCES installed_artifacts(id) ON DELETE CASCADE
);
```

The CHECK constraint pins the five v0.1 wire codes; adding a new category requires a new migration. The `ON DELETE CASCADE` means `chum uninstall` auto-removes the package's grants — no orphan rows.

### CLI surface

| Command | Effect | Wire codes on error |
|---|---|---|
| `chum permit <name> --grant <kind>=<value> [...]` | Add one or more grants. Idempotent — repeating is a no-op. | `unknown_permission` (parse) |
| `chum revoke <name> --grant <kind>=<value>` | Remove one grant. | `grant_not_found` |
| `chum permissions <name>` | Three-section diff: declared / granted / missing. | — |
| `chum start <name>` (existing) | Now subject to broker check; `permission_denied` if any declared permission is ungranted. | `permission_denied` |

`chum install` prints a hint listing the exact `chum permit` calls a user needs after install when the manifest declares any permissions. Skipped in `--json` mode (callers can read the manifest directly).

### v0.1 invariants and explicit deferrals

- **Pre-broker manifests still work.** Manifests without a `[permissions]` block parse to `Permissions::default()` (empty), broker auto-allows. No migration of existing installs needed.
- **Extra grants beyond declared are silently allowed.** The broker validates "are required permissions covered?", not "are grants minimal?" `TODO(chum-v0.2)` marker in `chum-broker/src/lib.rs::validate` flags the strict-grants follow-up.
- **No path canonicalisation on grants.** `~/Documents` is stored literally and won't match `/Users/x/Documents`. Helpful errors > silent expansion in v0.1.
- **Puppeteer is the documented gap.** Its real requirements (broad network access, Chromium subprocess) need wildcard / prefix matching. `chum-puppeteer.toml` ships with empty `[permissions]` and an inline comment pointing at v0.2.
- **v0.2 seam:** `chum_broker::sandbox_profile(&Permissions, &[Grant]) -> SandboxProfile` is the planned next function — generates a `sandbox-exec` `.sb` file body from the granted permissions. The daemon's `spawn_child` will wrap `Command::new(cmd)` with `sandbox-exec -p <profile>`. v0.1 surface stays unchanged when v0.2 lands.

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
