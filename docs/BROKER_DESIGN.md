# chum-broker — v0.1 design

> **Status:** Proposal. Sign off required before any implementation lands.
> **Owner:** This document is the source of truth for what v0.1 broker does and does not do. Discrepancies with code are bugs in the code.

## 1. Wedge

The broker is the security primitive that differentiates CHUM from every other MCP package manager. v0.1 ships *capability bookkeeping* — manifests declare what they need, users grant what they're willing to give, the daemon refuses to spawn if the two don't match. **Spawned processes are not actually sandboxed in v0.1.** Real enforcement (sandbox-exec on macOS, env scrubbing, network filtering) is v0.2's job; v0.1 establishes the trust-verification surface that v0.2 enforces.

This split is the only way the v0.1 broker ships in a reasonable scope. "Trust verification" is a one-week job; "trust enforcement" is a multi-month job involving Apple Endpoint Security, sandbox-exec profile templates, and a permission UX iteration loop. Both ship; v0.1 ships the half that locks the surface.

## 2. Permission categories (v0.1)

Four categories — chosen because they cover what every first-party manifest currently needs, and they map cleanly to v0.2's enforcement primitives. Each category has exactly one or two operations and a single representable value type (a string).

| Category | Subkey | Value | Example |
|---|---|---|---|
| `filesystem` | `read` | absolute path | `/Users/x/Documents` |
| `filesystem` | `write` | absolute path | `/tmp/chum-workspace` |
| `network` | `outbound` | host (no scheme, no port) | `api.search.brave.com` |
| `env` | `read` | env var name | `BRAVE_API_KEY` |
| `subprocess` | `exec` | program name or absolute path | `git`, `/usr/bin/curl` |

**Granularity rule for v0.1: exact-string match.** A grant for `/Users/x` does NOT cover `/Users/x/Documents` — they have to be granted separately. Wildcards (`*.anthropic.com`), prefix matching, and path canonicalization land in v0.2. Reasoning: v0.1 bookkeeping is about *did the user agree to this exact thing?* — fuzzy matching makes the audit story worse and adds parsing complexity for no v0.1 benefit. v0.2's real enforcement layer needs prefix / wildcard for usability reasons (you can't list every file path explicitly), but v0.2 also has the security model that justifies the fuzziness.

**Five permission kinds, named as one string each**: `filesystem.read`, `filesystem.write`, `network.outbound`, `env.read`, `subprocess.exec`. These are the wire codes used in the IPC layer, the registry table's CHECK constraint, and the cli's `--grant` argument format (`<kind>=<value>`).

Permission categories not in v0.1, with rationale:

- **`device.*`** (camera, microphone, location) — needed for some MCP servers but not any first-party manifest in v0.1. Add when one ships.
- **`keychain.*`** (macOS Keychain access) — `chum env` already covers the secrets surface; Keychain is the v0.2 backing store, not a separate category.
- **`port.bind`** (local network listen) — HTTP/SSE MCP servers will need this, but v0.1 only ships stdio servers, so no first-party manifest needs it. Add with the first HTTP server manifest.

## 3. Manifest [permissions] schema

Replaces the v0.1 `Option<toml::Table>` placeholder in `chum_core::Manifest`. Typed across the board so the broker can match without parsing strings.

```toml
[permissions]
# Every subtable is optional and defaults to empty.

[permissions.filesystem]
read  = ["/Users/x/Documents"]
write = ["/tmp/chum-workspace"]

[permissions.network]
outbound = ["api.search.brave.com", "api.anthropic.com"]

[permissions.env]
read = ["BRAVE_API_KEY", "HOME"]

[permissions.subprocess]
exec = ["git", "/usr/bin/curl"]
```

Rust types (in `chum_core::manifest::permissions`):

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    #[serde(default)] pub filesystem: FilesystemPermissions,
    #[serde(default)] pub network: NetworkPermissions,
    #[serde(default)] pub env: EnvPermissions,
    #[serde(default)] pub subprocess: SubprocessPermissions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemPermissions {
    #[serde(default)] pub read: Vec<String>,
    #[serde(default)] pub write: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPermissions {
    #[serde(default)] pub outbound: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvPermissions {
    #[serde(default)] pub read: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubprocessPermissions {
    #[serde(default)] pub exec: Vec<String>,
}
```

Three properties of this shape that matter:

1. **`Permissions::default()` is empty across the board.** Pre-broker manifests (no `[permissions]` block) parse to `Permissions::default()`. The broker sees zero required permissions, matches against zero granted permissions, returns `Allow`. Old installs still work.
2. **`#[serde(deny_unknown_fields)]`** at every level. A typo like `[permissions.filesystem.exec]` is a parse error, not a silent skip. Cheap insurance.
3. **`Vec<String>` for every leaf.** No nested objects, no version qualifiers, no Optional<String>. The cli's `--grant <kind>=<value>` parser matches exactly the wire format, no transformations.

The internal "permission requirement" type is a flat `(PermissionKind, String)` pair:

```rust
pub enum PermissionKind {
    FilesystemRead,
    FilesystemWrite,
    NetworkOutbound,
    EnvRead,
    SubprocessExec,
}

pub struct PermissionRequirement {
    pub kind: PermissionKind,
    pub value: String,
}

impl Permissions {
    pub fn iter_requirements(&self) -> impl Iterator<Item = PermissionRequirement> + '_ { ... }
}
```

The flat form is what the broker matches against grants. `iter_requirements()` yields one item per (kind, value) pair so the validate loop is a simple double iteration.

## 4. Grant storage

New `permission_grants` table in `chum-registry`. Migration `2_add_permission_grants`:

```sql
CREATE TABLE permission_grants (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    artifact_id INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
                    'filesystem.read', 'filesystem.write',
                    'network.outbound',
                    'env.read',
                    'subprocess.exec'
                )),
    value       TEXT NOT NULL,
    granted_at  TEXT NOT NULL,   -- RFC3339 UTC
    UNIQUE(artifact_id, kind, value),
    FOREIGN KEY (artifact_id) REFERENCES installed_artifacts(id) ON DELETE CASCADE
);
CREATE INDEX permission_grants_artifact ON permission_grants(artifact_id);
```

Notes:

- **`ON DELETE CASCADE`** means uninstalling a package removes all its grants automatically — no orphaned rows. Requires `PRAGMA foreign_keys = ON`, which `Registry::open` already sets.
- **`UNIQUE(artifact_id, kind, value)`** means each grant is unique per package; repeated `chum permit` of the same triple is a no-op (returns the existing row's id), not an error.
- **`granted_at`** stamped by the registry at grant time, same pattern as `installed_at`. Records *when* the user said yes — useful for a future audit log + the v0.2 approval inbox.
- **`kind`** is a flat dotted string, not split into category/subkey columns. Matches the wire format the cli speaks; one column to query against.

New `Registry` methods (added to the existing public API):

```rust
impl Registry {
    pub fn grant(&self, artifact_id: i64, kind: &str, value: &str) -> Result<i64, RegistryError>;
    pub fn revoke(&self, artifact_id: i64, kind: &str, value: &str) -> Result<(), RegistryError>;
    pub fn list_grants(&self, artifact_id: i64) -> Result<Vec<Grant>, RegistryError>;
    pub fn list_grants_by_name_version(&self, name: &str, version: &str) -> Result<Vec<Grant>, RegistryError>;
}

pub struct Grant {
    pub kind: String,            // "filesystem.read" etc.
    pub value: String,
    pub granted_at: DateTime<Utc>,
}
```

`grant` is `INSERT OR IGNORE` semantically: repeated grants don't error; `list_grants` will only show one row per (kind, value) anyway thanks to `UNIQUE`.

`revoke` deletes the matching row; `RegistryError::NotFound` if there's no matching grant.

## 5. The broker library (`chum-broker`)

Pure validation library. **No I/O.** The crate's public surface is one function and two types:

```rust
// chum_broker

pub enum BrokerVerdict {
    Allow,
    Deny {
        unmet: Vec<chum_core::PermissionRequirement>,
    },
}

pub fn validate(
    required: &chum_core::Permissions,
    granted: &[chum_registry::Grant],
) -> BrokerVerdict {
    let granted_set: HashSet<(&str, &str)> = granted
        .iter()
        .map(|g| (g.kind.as_str(), g.value.as_str()))
        .collect();
    let unmet: Vec<_> = required
        .iter_requirements()
        .filter(|req| !granted_set.contains(&(req.kind.as_str(), req.value.as_str())))
        .collect();
    if unmet.is_empty() {
        BrokerVerdict::Allow
    } else {
        BrokerVerdict::Deny { unmet }
    }
}
```

That's the whole v0.1 broker. ~20 lines of real logic. The reason it's a separate crate (rather than sitting in chum-core) is that v0.2's sandbox-profile-generator (`fn sandbox_profile(...) -> String` producing sandbox-exec `.sb` content) needs a home, and chum-core's "no I/O, no logic, just types" boundary should hold. The broker is where capability *policy* lives; chum-core is where capability *schema* lives.

`chum-broker` depends on `chum-core` (for `Permissions`) and `chum-registry` (for `Grant`). No async, no tokio, no I/O.

## 6. Daemon integration (`chum-daemon`)

The `spawn` IPC verb gets one new step between manifest re-parse and `Supervisor::spawn`:

```
verb_spawn (chum-daemon)
   │
   ▼
1. registry.get_by_name_version  ──► InstalledArtifact { id, ... }
   │
   ▼
2. fs::read_to_string(install_dir/chum-manifest.toml)
   │
   ▼
3. chum_core::parse_and_validate ──► Manifest { permissions, ... }
   │
   ▼
4. registry.list_grants(artifact.id) ──► Vec<Grant>
   │
   ▼
5. chum_broker::validate(&manifest.permissions, &grants)
   │
   ▼ Allow                          ▼ Deny { unmet }
   │                                │
   ▼                                ▼
6. Supervisor::spawn                Response::error("permission_denied",
                                       message naming the unmet requirements)
```

New wire code: `permission_denied`. The error message lists the unmet requirements so the user knows exactly which `chum permit` calls to run. No partial-spawn — if any required permission is unmet, refuse the entire spawn.

`process_status` is unchanged. `list_processes` is unchanged. Only `spawn` (and `restart`, which calls `spawn` internally) get the broker check.

## 7. CLI surface

Three new subcommands. All follow the same `<name> [--version V] [--root] [--socket-path] [--json]` shape used by the existing lifecycle commands.

### `chum permit <name> --grant <kind>=<value> [--grant ...]`

Grant one or more permissions to an installed package. Multiple `--grant` flags accumulate.

```sh
chum permit filesystem --grant filesystem.read=/Users/x/Documents
chum permit brave-search \
    --grant network.outbound=api.search.brave.com \
    --grant env.read=BRAVE_API_KEY
```

Grant string format: `<kind>=<value>` where `<kind>` is one of the five known strings. Parse error → `unknown_permission` code.

Output:
- Human: `Granted to <name> <version>: <kind>=<value> (+ 2 more)`
- JSON: `{"status":"ok","granted":[{"kind":"...","value":"..."}, ...]}`

If the grant already existed, it's a no-op (returns the existing row's data, no error).

### `chum revoke <name> --grant <kind>=<value>`

Inverse of permit. One grant per invocation in v0.1.

Output:
- Human: `Revoked from <name> <version>: <kind>=<value>`
- JSON: `{"status":"ok","revoked":{"kind":"...","value":"..."}}`

If the grant doesn't exist → `NotFound { kind, value }`. (Renamed from generic NotFound for clarity in the cli error envelope.)

### `chum permissions <name> [--version V] [--json]`

Three-section diff:

```
chum-filesystem 0.1.0

Declared by manifest:
  filesystem.read   /Users/x/Documents
  filesystem.write  /tmp/chum-workspace
  env.read          HOME

Granted:
  filesystem.read   /Users/x/Documents

Missing (would block `chum start`):
  filesystem.write  /tmp/chum-workspace
  env.read          HOME
```

JSON form (script-friendly):

```json
{
  "status": "ok",
  "permissions": {
    "name": "chum-filesystem",
    "version": "0.1.0",
    "declared": [
      {"kind": "filesystem.read",  "value": "/Users/x/Documents"},
      {"kind": "filesystem.write", "value": "/tmp/chum-workspace"},
      {"kind": "env.read",         "value": "HOME"}
    ],
    "granted":  [{"kind": "filesystem.read", "value": "/Users/x/Documents", "granted_at": "2026-05-21T13:30:00Z"}],
    "missing":  [
      {"kind": "filesystem.write", "value": "/tmp/chum-workspace"},
      {"kind": "env.read",         "value": "HOME"}
    ]
  }
}
```

The `missing` field is what makes `chum permissions` the diagnostic command users will actually need — it's "what's blocking `chum start <name>`?"

### `chum start` UX after broker lands

Today `chum start <name>` succeeds if the package is installed. After broker:
- If `chum permissions` shows `missing` is non-empty → `chum start` fails with `permission_denied` and the same list of unmet requirements.
- Manifest with no `[permissions]` block → empty declared list → always passes the broker.

No automatic grant prompt at `chum start`. The user runs `chum permit` first, deliberately. This is the entire v0.1 trust wedge: every grant is a recorded, deliberate user action — not a one-click "yes to all."

## 8. New error variants

In `chum-cli` (`UserFacingError`):

| Variant | Code | Renders as |
|---|---|---|
| `PermissionDenied { name, version, unmet }` | `permission_denied` | `'foo' 0.1.0 needs grants not yet given: filesystem.read=/x, env.read=Y. Run: chum permit foo --grant ... --grant ...` |
| `UnknownPermission { input }` | `unknown_permission` | `'<input>' is not a known permission. Expected '<kind>=<value>' where kind is one of: filesystem.read, filesystem.write, network.outbound, env.read, subprocess.exec` |
| `GrantNotFound { name, version, kind, value }` | `grant_not_found` | `no grant '<kind>=<value>' on '<name>' <version>` |

In `chum-daemon` IPC codes:

| Code constant | Wire string |
|---|---|
| `PERMISSION_DENIED` | `permission_denied` |

## 9. First-party manifest updates

All ten manifests in `manifests/` need a `[permissions]` block reflecting their actual capability needs. Indicative shapes (subject to upstream-README verification at implementation time):

- **chum-filesystem** — `filesystem.read = [<path>]`, `filesystem.write = [<path>]` (matching `runtime.args[2]`). The user edits the path before install; the manifest grants the user can match exactly.
- **chum-brave-search** — `network.outbound = ["api.search.brave.com"]`, `env.read = ["BRAVE_API_KEY"]`.
- **chum-sequential-thinking** — empty `[permissions]`.
- **chum-memory** — `filesystem.read/write = [<MEMORY_FILE_PATH or default>]`.
- **chum-puppeteer** — `filesystem.read = ["~/.cache/puppeteer"]`, `subprocess.exec = ["chrome", "chromium"]` (Chromium binary), `network.outbound = ["*"]` — but `*` doesn't work in v0.1 (exact-match). Either grant nothing for puppeteer's network and accept that v0.1 can't enforce or block; or skip puppeteer from the permitted set this session. **Recommendation: leave puppeteer's permissions empty for v0.1** with a comment saying "puppeteer needs broad network + Chromium subprocess; v0.2 wildcards will make this expressible. v0.1 is an exception — the manifest declares zero permissions and the broker waves it through. Document the gap rather than fake-encode it."
- **chum-postgres** — `network.outbound = [<host from connection string>]`. The host comes out of the connection-string positional arg.
- **chum-slack** — `network.outbound = ["slack.com"]`, `env.read = ["SLACK_BOT_TOKEN", "SLACK_TEAM_ID"]`.
- **chum-github** — `network.outbound = ["api.github.com"]`, `env.read = ["GITHUB_PERSONAL_ACCESS_TOKEN"]`.
- **chum-redis** — `network.outbound = ["localhost"]` (or as-edited).
- **chum-everything** — empty `[permissions]`.

Each manifest gains a header-comment paragraph explaining what `chum permit` calls the user will need to run after installing, mirroring the existing "EDIT BEFORE INSTALL" pattern.

## 10. What v0.2 adds (the seam)

v0.1 ships `chum_broker::validate(&Permissions, &[Grant]) -> BrokerVerdict`. v0.2 adds:

1. **`chum_broker::sandbox_profile(&Permissions, &[Grant]) -> SandboxProfile`** — returns a `String` (sandbox-exec `.sb` file body) derived from the granted permissions. Default-deny with explicit allows for each granted permission.
2. **Daemon integration**: the supervisor's `spawn_child` wraps `tokio::process::Command::new(cmd)` with `sandbox-exec -p <profile_path> <cmd>`. The profile path is per-process, written to `<install_dir>/sandbox.sb` at spawn time.
3. **Env scrubbing**: the spawned process's env is filtered to only the names in `permissions.env.read`; everything else is removed before `exec`. This is *enforcement* of the same thing v0.1 *records*.
4. **Wildcard / prefix matching** for filesystem paths and network hosts. The manifest schema doesn't change (still `Vec<String>`); the *interpretation* changes from exact-match to prefix-match for `filesystem.*` and glob-match for `network.outbound`. v0.1 manifests stay valid; the broker just becomes more lenient about what counts as a match.
5. **Audit log**: every grant check (allow or deny) writes a row to a new `permission_audit` table. Useful for incident response — "what did this MCP server try to access yesterday?"
6. **`chum permit --interactive`**: prompt for each missing permission at install time, with a y/N answer per grant. Lower-friction UX once the trust model is real.

The v0.1 surface is the v0.2 surface plus enforcement — no schema changes, no IPC verb changes, no CLI command additions. v0.2 is "make the bookkeeping mean something."

## 11. Open questions (call out before implementation)

1. **Should `chum install` show declared permissions in its output?** Today `chum install` just confirms the install. A "this package will need the following permissions; grant them with `chum permit ...`" hint at install time would shrink the discovery-of-broker UX gap. I lean **yes** (add to the install confirmation envelope). Verify before implementing.

2. **Path canonicalisation on grants.** `chum permit foo --grant filesystem.read=~/Documents` — should the cli expand `~` and resolve symlinks before storing? v0.1 says **no** (store the literal string the user typed; exact-match against the manifest's literal string). v0.2's enforcement layer will need canonicalisation but can do it at sandbox-profile generation time, not at grant time. Document.

3. **What does `chum start` do when grants exist but `chum permissions` shows extras (granted but not declared)?** I lean **silently allow** (extra grants are a user choice, not an error). Document.

4. **What happens if the manifest's `[permissions]` declares a permission that contradicts itself (e.g. the same path in `read` and `write`)?** v0.1: **no contradiction check** — both grants are issued separately, the user permits both separately. Document.

5. **CLI shorthand — is `chum permit foo --grant fs.read=/x` worth supporting (`fs` → `filesystem`)?** I lean **no** for v0.1. Five strings are easy enough. Re-evaluate if user research surfaces it.

## 12. Implementation order (post-approval)

1. **C1 `feat(core): typed Permissions struct in manifest schema`** — `chum-core::manifest::permissions` module, replaces the `Option<toml::Table>` placeholder. Unit tests for round-trip TOML, empty defaults, `iter_requirements` iteration.

2. **C2 `feat(registry): permission_grants table + grant/revoke/list_grants methods`** — migration 2, four new public methods. Integration tests for grant uniqueness, revoke, cascade-delete on uninstall.

3. **C3 `feat(broker): validate(Permissions, &[Grant]) -> BrokerVerdict`** — replace the chum-broker stub with the real lib. Pure-function tests over a table of (permissions, grants) → expected verdict cases.

4. **C4 `feat(daemon): broker check in spawn pipeline`** — wire `chum_broker::validate` into `verb_spawn`. New `permission_denied` wire code. Integration test asserts `chum start` against an unpermitted manifest fails with the right code.

5. **C5 `feat(cli): chum permit/revoke/permissions subcommands`** — three new commands. Three new `UserFacingError` variants. Integration tests covering: permit-then-start succeeds, revoke-then-start fails, permissions diff output shape.

6. **C6 `chore(manifests): declare actual permissions on first-party manifests`** — update the 10 manifests with the indicative shapes above (skipping puppeteer per Section 9). Update `first_party_manifests.rs` test to assert each declared permission parses cleanly.

7. **C7 `docs: README + ARCHITECTURE for broker + permissions UX`** — README gains a "Granting permissions" section showing the install → permit → start flow. ARCHITECTURE.md grows a broker module section. CLAUDE.md gets a one-line note that v0.1 broker is bookkeeping-only.

Final step (outside Phase 4): tag `v0.1.0-alpha.6`.

7 commits for Phase 4. None bundle unrelated changes; each is bisectable.

---

*Approval gate: review this design and respond with `go broker` (or pushback that triggers revision) before implementation starts.*
