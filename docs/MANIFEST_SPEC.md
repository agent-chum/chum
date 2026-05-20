# CHUM Manifest Specification — v0.1

**Status:** v0.1, implemented by `chum-core` v0.0.1.
**Schema version key:** `schema_version = "0.1"`.

A CHUM manifest is a TOML file describing one installable MCP server. Manifests live under [`/manifests/`](../manifests/) in this repo for first-party packages and on the public registry for third-party packages.

> The reference Rust types live in [`chum-core::manifest`](../crates/chum-core/src/manifest/mod.rs). The parser and validator are [`chum-core::manifest::parse_str`](../crates/chum-core/src/manifest/parse.rs) and [`chum-core::manifest::validate`](../crates/chum-core/src/manifest/validate.rs). The integration tests in [`crates/chum-core/tests/`](../crates/chum-core/tests/) are the canonical "does this manifest work?" reference.

## File naming

`<name>.toml` (unprefixed). The `/manifests/` directory acts as its own namespace; the package's own `name` field is what CHUM uses internally.

## Top-level fields

| Field | Type | Required | Notes |
|---|---|---|---|
| `schema_version` | string | yes | Must be `"0.1"` for this build of chum-core. Newer versions are rejected at parse time. |
| `[package]` | table | yes | Identity metadata. |
| `[source]` | table | yes | Where to install from. Tagged enum — see [Source kinds](#source-kinds). |
| `[runtime]` | table | yes | How to start the server. |
| `[lifecycle]` | table | no | Restart / startup-timeout / shutdown-grace. Defaults applied if omitted. |
| `[health]` | table | no | Health-check strategy. Defaults to `kind = "process"`. |
| `[capabilities]` | table | no | Declared tools / resources / prompts. Informational in v0.1; enforced in v0.2. |
| `[permissions]` | table | no | Forward-compat placeholder. v0.2 will define a schema; v0.1 accepts arbitrary content and ignores it. |
| `[signature]` | table | no | Forward-compat placeholder. v0.3 will define signing fields; v0.1 accepts arbitrary content and ignores it. |

Unknown top-level keys are rejected (`deny_unknown_fields`). Forward compatibility is enforced via `schema_version`, not by silently tolerating unknown fields.

## `[package]`

| Field | Type | Required | Notes |
|---|---|---|---|
| `name` | string | yes | Must match `[a-z][a-z0-9-]{0,62}`. Lowercase ASCII, digits, and dashes; starts with a letter; 1–63 characters. |
| `version` | string | yes | Semver-ish. Non-empty. Not strictly validated in v0.1 — a `semver` parser may land in v0.2. |
| `description` | string | yes | Human-readable summary. |
| `license` | string | yes | SPDX-ish identifier. MIT, Apache-2.0, and BSD-style are accepted ecosystem-wide; CHUM does not enforce a list. |
| `authors` | array of strings | yes | Author / maintainer list. May be empty. |
| `tags` | array of strings | no | Free-form. Defaults to `[]`. |

## `[source]`

`source.kind` selects the install method. Each kind requires its own fields.

### Source kinds

#### `kind = "npm"`

```toml
[source]
kind = "npm"
package = "@modelcontextprotocol/server-filesystem"
version = "^0.1"
```

| Field | Type | Required |
|---|---|---|
| `package` | string | yes |
| `version` | string | yes — semver range, including `^X.Y` / `~X.Y` / pinned `X.Y.Z` |

#### `kind = "pypi"`

```toml
[source]
kind = "pypi"
package = "mcp-server-foo"
version = "^0.2"
```

| Field | Type | Required |
|---|---|---|
| `package` | string | yes |
| `version` | string | yes |

The installer chooses `uvx` / `pipx` / `pip` based on the package; the manifest does not pin a runner in v0.1.

#### `kind = "github"`

```toml
[source]
kind = "github"
repo = "owner/repo"
rev = "v1.2.3"
subdir = "servers/foo"   # optional
```

| Field | Type | Required |
|---|---|---|
| `repo` | string (`owner/repo`) | yes |
| `rev` | string (commit / tag / branch) | yes |
| `subdir` | string | no |

#### `kind = "local"`

```toml
[source]
kind = "local"
path = "/Users/karoshi/code/mcp-experiment"
```

| Field | Type | Required |
|---|---|---|
| `path` | string | yes |

For development manifests pointing at a working tree. Path resolution is install-layer concern; chum-core stores literally.

#### `kind = "binary"`

```toml
[source]
kind = "binary"
url = "https://github.com/example/mcp-sqlite/releases/download/v0.2.0/mcp-sqlite-aarch64-apple-darwin.tar.gz"
checksum_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
target_triple = "aarch64-apple-darwin"   # optional
```

| Field | Type | Required | Validation |
|---|---|---|---|
| `url` | string | yes | Must start with `http://` or `https://`. |
| `checksum_sha256` | string | yes | Exactly 64 hex characters (either case accepted). |
| `target_triple` | string | no | Reserved for multi-platform releases. |

**Archive detection (v0.1, install layer).** [`chum-install`](../crates/chum-install/) decides whether to extract or place verbatim by looking at the URL suffix only. Query strings (`?token=…`) are stripped before matching.

| URL suffix | Treatment |
|---|---|
| `.tar.gz` / `.tgz` | tar + gzip extraction into `<install_dir>/bin/` |
| `.tar` | tar extraction into `<install_dir>/bin/` |
| `.zip` | zip extraction into `<install_dir>/bin/` |
| any other suffix | placed verbatim at `<install_dir>/bin/<basename>` |

v0.1 does **not** inspect `Content-Type` headers or magic bytes. Manifest authors must use a known extension to trigger extraction; an unknown extension yields a single-file install.

The `Registry` source kind lands in v0.5 behind a `schema_version` bump.

## `[runtime]`

| Field | Type | Required | Notes |
|---|---|---|---|
| `command` | string | yes | Executable to invoke. |
| `args` | array of strings | no | Defaults to `[]`. Strings are **literal** — chum-core does not substitute `${VAR}` syntax; templating lives in the install layer. |
| `transport` | table | yes | Tagged enum — see below. |
| `env` | table of string→string | no | `BTreeMap` semantics — deterministic order. Defaults to `{}`. |

### Transport kinds

#### `kind = "stdio"`

```toml
[runtime.transport]
kind = "stdio"
```

No additional fields. The server is spawned and supervised as a child of `chum-daemon`.

#### `kind = "http"` / `kind = "sse"`

```toml
[runtime.transport]
kind = "http"
port = 8080
bind = "127.0.0.1"   # default; never 0.0.0.0
path = "/mcp"        # optional
```

| Field | Type | Required | Notes |
|---|---|---|---|
| `port` | integer (`u16`) | yes | |
| `bind` | string | no | Default `127.0.0.1`. Validated: `0.0.0.0`, `::`, `[::]`, `*` are rejected. **CHUM is local-first; servers must bind to loopback.** |
| `path` | string | no | URL path component. |

## `[lifecycle]`

| Field | Type | Default | Notes |
|---|---|---|---|
| `restart` | string enum | `"on-failure"` | One of `always` / `on-failure` / `never`. Wire format is kebab-case. |
| `startup_timeout_sec` | integer | `10` | Seconds to wait for the server to come up. |
| `shutdown_grace_sec` | integer | `5` | Seconds to wait for graceful shutdown before SIGKILL. |

The entire `[lifecycle]` table can be omitted; defaults apply.

## `[health]`

`health.kind` selects the check strategy. Only `process` is wired in v0.1 — `ping` and `http` parse and round-trip, but `chum-daemon` does not execute them until v0.x lands the active health-check runner.

```toml
[health]
kind = "process"
```

```toml
[health]
kind = "ping"
interval_sec = 30
timeout_sec = 5
```

```toml
[health]
kind = "http"
url = "http://127.0.0.1:8080/health"
expect_status = 200
interval_sec = 30
timeout_sec = 5
```

## `[capabilities]`

Informational in v0.1. The broker enforces these in v0.2.

```toml
[capabilities]
tools     = ["read_file", "write_file"]
resources = []
prompts   = []
```

| Field | Type | Default |
|---|---|---|
| `tools` | array of strings | `[]` |
| `resources` | array of strings | `[]` |
| `prompts` | array of strings | `[]` |

## Forward-compat placeholders

### `[permissions]` (v0.2)

Accepted by v0.1 chum-core as an arbitrary TOML table; not interpreted. v0.2 will replace this with a typed schema (per-tool grants, path allowlists, network zones) behind a `schema_version` bump.

```toml
# v0.1 chum-core stores this verbatim and round-trips it. v0.2 will parse it.
[permissions]
allowed_paths = ["${HOME}/Documents"]
denied_paths  = ["${HOME}/.ssh"]
```

### `[signature]` (v0.3)

Accepted by v0.1 chum-core as an arbitrary TOML table; not interpreted. v0.3 will replace this with Sigstore-compatible signature fields behind a `schema_version` bump.

## Validation rules

`chum-core::manifest::validate` runs semantic checks on top of the structural TOML parse. The structural parse catches:

- TOML syntax errors
- Unknown top-level fields
- Missing required fields (e.g. a `Source::Binary` with no `checksum_sha256`)
- Wrong-typed fields

Semantic validation adds:

- `package.name` matches `[a-z][a-z0-9-]{0,62}`
- `package.version` is non-empty
- `Source::Binary.url` starts with `http://` or `https://`
- `Source::Binary.checksum_sha256` is exactly 64 hex characters
- `Transport::Http.bind` and `Transport::Sse.bind` are not `0.0.0.0`, `::`, `[::]`, or `*`

Validation short-circuits on the first failure. Tooling that needs to report every problem at once should iterate manifests and collect errors per call.

### Error types

The Rust API returns `Result<Manifest, ManifestError>` from both `parse_str` and `validate`. Variants:

| Variant | Source |
|---|---|
| `Toml(toml::de::Error)` | TOML syntax error or missing required field. |
| `TomlSerialize(toml::ser::Error)` | Serialisation failed (rare). |
| `MissingSchemaVersion` | Top-level `schema_version` absent. |
| `UnsupportedSchemaVersion(String)` | `schema_version` does not match this build. |
| `InvalidName(String)` | `package.name` failed the regex check. |
| `InvalidVersion(String)` | `package.version` was empty. |
| `InvalidChecksum(String)` | `checksum_sha256` was not 64 hex characters. |
| `InvalidUrl(String)` | URL did not start with `http://` or `https://`. |
| `InvalidBindAddress(String)` | HTTP/SSE bind was a wildcard or unspecified address. |

## Forward compatibility

CHUM is local-first OSS with a long target horizon. Forward compatibility is enforced **explicitly** through `schema_version`, not implicitly through field tolerance:

- Older chum-core encountering a newer manifest: rejects at parse time with `UnsupportedSchemaVersion`.
- Newer chum-core encountering an older manifest: parses if the older version is on the supported list; otherwise rejects.
- Within a `schema_version`, unknown fields are rejected (`deny_unknown_fields`).
- `[permissions]` and `[signature]` are explicitly carved out as freeform tables until their owning version lands; this lets future fields be authored before all v0.1 readers in the wild upgrade.

## Worked example

The reference manifest is [`crates/chum-core/tests/fixtures/chum-filesystem.toml`](../crates/chum-core/tests/fixtures/chum-filesystem.toml). It encodes Anthropic's official filesystem MCP server as it ships today (`npx -y @modelcontextprotocol/server-filesystem <path>`), declares its v0.2-shaped permissions for forward compatibility, and is what the parse / round-trip / validate test suite exercises end-to-end.

## What v0.1 explicitly defers

- **Template substitution.** `${HOME}` and similar tokens in `args` / `env` are stored literally. Substitution happens in the install/start layer, not in chum-core.
- **Active health checks.** `ping` and `http` health kinds are accepted and round-tripped; `chum-daemon` only executes `process` in v0.1.
- **Permissions enforcement.** v0.2 work.
- **Signature verification.** v0.3 work.
- **Registry source kind.** v0.5 work.
- **Multi-platform binary selection.** `target_triple` is parsed in v0.1 but the install layer does not yet choose between artefacts.
