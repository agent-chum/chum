//! Shared helpers for `chum-registry` integration tests.
//!
//! Lives under `tests/common/mod.rs` so Cargo treats it as a module
//! pulled in by sibling test binaries (`mod common;`), not as its own
//! integration binary. The `#![allow(dead_code)]` keeps the per-binary
//! dead-code lint quiet when one test file uses only a subset of the
//! helpers.

#![allow(dead_code)]

use std::path::PathBuf;

use chum_install::{InstalledArtifact, SourceKind};
use tempfile::TempDir;

/// A fresh tempdir plus the pre-computed `state.db` path inside it.
///
/// The file does not exist yet — `Registry::open` will create it.
/// Drop the struct to remove the directory and the database with it,
/// keeping each test fully isolated.
pub struct TestDb {
    /// The tempdir handle. Underscore-prefixed because tests reach
    /// for `path` directly; the field exists purely to keep the
    /// directory alive for the lifetime of the test.
    pub _dir: TempDir,
    /// Resolved `<tempdir>/state.db` path to hand to `Registry::open`.
    pub path: PathBuf,
}

impl TestDb {
    /// Allocate a new tempdir and compute `state.db` inside it.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("create tempdir");
        let path = dir.path().join("state.db");
        Self { _dir: dir, path }
    }
}

/// Build an `InstalledArtifact` with predictable shape for tests.
///
/// `install_dir` follows the documented layout
/// (`/tmp/chum-test/packages/<name>/<version>/`) and `entrypoint` is
/// derived per `SourceKind`. The paths are not created on disk — the
/// registry stores strings and doesn't probe the filesystem.
pub fn make_artifact(name: &str, version: &str, kind: SourceKind) -> InstalledArtifact {
    let install_dir = PathBuf::from(format!("/tmp/chum-test/packages/{name}/{version}"));
    let entrypoint = match kind {
        SourceKind::Npm => install_dir.join("node_modules").join(name),
        SourceKind::Local => install_dir.join("local-src"),
        SourceKind::Binary => install_dir.join("bin"),
        _ => install_dir.join("bin"),
    };
    InstalledArtifact {
        name: name.to_string(),
        version: version.to_string(),
        install_dir,
        entrypoint,
        source_kind: kind,
    }
}
