# CHUM — Coding Rules

This file is binding on every contribution. AI builders (Claude Code, Grok Build, etc.) and human contributors both apply these rules. If a rule conflicts with a request, the rule wins; flag the conflict instead of breaking the rule.

## Scope

This repo is a local-first MCP package manager and capability broker for AI agents on Apple Silicon. Public design lives in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Roadmap lives in [`ROADMAP.md`](ROADMAP.md).

## Language and style

- **Rust 2021 edition minimum.** Prefer 2024 when the toolchain supports it. The workspace currently sets `edition = "2024"` with `rust-version = "1.85"`.
- **No `unwrap()` or `expect()` in non-test code.** Use `?` with typed errors. Tests may use either, freely.
- **Errors: `thiserror` for library crates, `anyhow` only in binary crates at the edge.** `chum-core`, `chum-broker`, `chum-registry`, and `chum-daemon` return typed errors. `chum-cli` is the one place `anyhow::Result` is acceptable.
- **Async: tokio runtime.** Single-threaded where it suffices; reach for multi-threaded only with a real reason.
- **No `unsafe` blocks without an explicit justification comment.** `#![forbid(unsafe_code)]` is the default at the crate root unless a justified exception exists.
- **Public API items must have rustdoc comments.** `#![warn(missing_docs)]` at the crate root.

## Testing

- Every public function in `chum-core` needs unit tests.
- Integration tests live in `crates/<crate>/tests/`.
- `cargo test --workspace` must pass before any commit.

## Dependencies

- **Justify every new dependency in the commit message.** Why this one, why not std, why not an existing dep.
- Prefer std and well-known crates (tokio, clap, serde, thiserror, anyhow, rusqlite).
- **MIT, Apache-2.0, or BSD licenses only.** No GPL.
- Centralised in `[workspace.dependencies]`; member crates opt in with `dep.workspace = true`.

## Architecture invariants

These are enforced at review time. Violation is a blocker.

- **`chum-core` has no I/O.** Pure types, schemas, and parsing only. No filesystem, network, process, or launchd touches.
- **`chum-cli` is a thin layer over the chum-daemon protocol once chum-daemon exists.** It never bypasses the daemon to talk to MCP servers or the manifest store directly. **v0.1 stopgap:** until the daemon protocol lands, `chum-cli` composes `chum-install` + `chum-registry` directly inside `commands/install.rs`; that composition moves behind the daemon when it ships. Do not add new direct-bypass paths beyond what is already there.
- **`chum-daemon` owns process supervision and state.** All `start` / `stop` / `restart` paths flow through it.
- **`chum-broker` gates all agent ↔ MCP server access.** No direct passthrough; every capability use is mediated.
- **`chum-registry` is read-write SQLite.** It never mixes concerns with `chum-broker`.

## Commits

- **Conventional commits:** `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`.
- **One logical change per commit.** The body explains *why*, not *what*.
- Do not bundle unrelated changes into a single commit.

## What this repo does NOT do

- **No Windows support, ever.** Out of scope forever.
- **No cloud-only features.** Local-first is the entire wedge.
- **No agent framework.** CHUM is a control plane around frameworks, not one of them.
- **No premature optimisation.** v0.1 is correctness; performance comes later.
