//! Shared helpers for chum-cli integration tests.
//!
//! Lives under `tests/common/mod.rs` so Cargo treats it as a module
//! consumed via `mod common;` from sibling test binaries rather than
//! its own integration binary. `#![allow(dead_code)]` keeps the
//! per-binary dead-code lint quiet when one test file uses only a
//! subset of the helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// Fixture template path relative to the test's `CARGO_MANIFEST_DIR`.
/// Substituting `__CHUM_LOCAL_PATH__` with an absolute path yields a
/// runnable Source::Local manifest.
const FIXTURE_TEMPLATE: &str =
    include_str!("../fixtures/chum-local-test.toml.template");

const PLACEHOLDER: &str = "__CHUM_LOCAL_PATH__";

/// One tempdir holding: a stub local-source directory, a rendered
/// manifest pointing at it, and a fresh CHUM root. Drop the value to
/// clean everything up.
pub struct Scratch {
    /// Underlying tempdir handle. Underscore-prefixed because tests
    /// reach for `chum_root` / `manifest_path` directly; this field
    /// exists purely to keep the dir alive for the test's lifetime.
    pub _dir: TempDir,
    /// Canonical absolute path to the symlink target.
    pub local_src: PathBuf,
    /// Path to the rendered manifest TOML.
    pub manifest_path: PathBuf,
    /// CHUM root for this test (passed via `--root`).
    pub chum_root: PathBuf,
}

impl Scratch {
    /// Allocate a fresh Scratch with the manifest rendered and the
    /// stub local-src populated. The CHUM root is not created — the
    /// install pipeline will mkdir it.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let local_src = dir.path().join("local-src");
        std::fs::create_dir_all(&local_src).expect("create local-src");
        std::fs::write(local_src.join("SENTINEL"), b"chum test sentinel\n")
            .expect("write sentinel");
        let canonical = local_src.canonicalize().expect("canonicalize local-src");

        let rendered = FIXTURE_TEMPLATE.replace(PLACEHOLDER, &canonical.display().to_string());
        let manifest_path = dir.path().join("manifest.toml");
        std::fs::write(&manifest_path, rendered).expect("write rendered manifest");

        let chum_root = dir.path().join("chum-home");

        Self {
            _dir: dir,
            local_src: canonical,
            manifest_path,
            chum_root,
        }
    }
}

/// Path to the compiled `chum` binary, set by Cargo for integration
/// tests against the bin target.
pub fn chum_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chum")
}

/// Spawn the `chum` binary with `args` and return the completed
/// `Output`. Panics if the spawn itself fails (not if the process
/// exits non-zero — that is up to the caller to assert).
pub fn run_chum(args: &[&str]) -> Output {
    Command::new(chum_bin())
        .args(args)
        .output()
        .expect("spawn chum binary")
}

/// Stringify a `Path` for clap's positional arg consumption.
pub fn path_str(p: &Path) -> String {
    p.display().to_string()
}
