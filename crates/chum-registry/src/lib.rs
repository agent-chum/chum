//! `chum-registry` — local manifest store. SQLite-backed.
//!
//! Per the architecture invariants in `CLAUDE.md`:
//! - Read-write SQLite is the storage backend. `rusqlite` is the planned
//!   driver when real types land.
//! - **It never mixes concerns with `chum-broker`.** Storage of
//!   manifests, version pins, and dependency graph only. Permissions and
//!   secrets live elsewhere.
//!
//! This is the v0.0.1 scaffold — only `placeholder()` exists. Schema,
//! migrations, and the query API arrive alongside the manifest spec.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder export so the crate compiles before real types land.
///
/// Will be removed in the first feature commit that introduces an actual
/// store type.
pub fn placeholder() {}
