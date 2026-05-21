//! End-to-end integration tests for `chum install`.
//!
//! Each test spawns the `chum` binary via the Cargo-provided
//! `CARGO_BIN_EXE_chum` env var (no `assert_cmd` dependency), points
//! it at a freshly-rendered Source::Local manifest, and asserts both
//! the filesystem state under `--root` and the registry row that
//! results.
//!
//! ZERO network — Source::Local symlinks a local path; there is no
//! HTTP or subprocess work in the v0.1 install pipeline for the
//! local kind.

use std::path::{Path, PathBuf};
use std::process::Command;

use chum_install::SourceKind;
use chum_registry::Registry;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str =
    include_str!("fixtures/chum-local-test.toml.template");

const PLACEHOLDER: &str = "__CHUM_LOCAL_PATH__";

/// One tempdir holding: the local-source directory, the rendered
/// manifest, and a fresh CHUM root. Drop to clean up everything.
struct Scratch {
    _dir: TempDir,
    local_src: PathBuf,
    manifest_path: PathBuf,
    chum_root: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let local_src = dir.path().join("local-src");
        std::fs::create_dir_all(&local_src).expect("create local-src");
        std::fs::write(local_src.join("SENTINEL"), b"chum test sentinel\n")
            .expect("write sentinel");

        // Canonicalise so the substituted path passes the install
        // crate's absolute-path / no-`..` validation cleanly.
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

fn chum_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chum")
}

fn run_chum(args: &[&str]) -> std::process::Output {
    Command::new(chum_bin())
        .args(args)
        .output()
        .expect("spawn chum binary")
}

fn manifest_path_str(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn install_local_fixture_end_to_end() {
    let scratch = Scratch::new();
    let out = run_chum(&[
        "install",
        &manifest_path_str(&scratch.manifest_path),
        "--root",
        &manifest_path_str(&scratch.chum_root),
    ]);

    assert!(
        out.status.success(),
        "chum install failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Installed chum-local-test 0.1.0"),
        "missing confirmation in stdout: {stdout}"
    );

    // Filesystem: install_dir contains the symlink pointing at the
    // canonical local-src.
    let install_dir = scratch
        .chum_root
        .join("packages")
        .join("chum-local-test")
        .join("0.1.0");
    let symlink = install_dir.join("local-src");
    assert!(install_dir.is_dir(), "install_dir missing: {install_dir:?}");
    let symlink_meta = std::fs::symlink_metadata(&symlink).expect("symlink metadata");
    assert!(
        symlink_meta.file_type().is_symlink(),
        "expected symlink at {symlink:?}"
    );
    let target = std::fs::read_link(&symlink).expect("read_link");
    assert_eq!(target, scratch.local_src);

    // Registry: row present with the right fields.
    let registry =
        Registry::open(scratch.chum_root.join("state.db")).expect("reopen registry");
    let row = registry
        .get_by_name_version("chum-local-test", "0.1.0")
        .expect("registry row");
    assert_eq!(row.name, "chum-local-test");
    assert_eq!(row.version, "0.1.0");
    assert_eq!(row.install_dir, install_dir);
    assert_eq!(row.entrypoint, symlink);
    assert_eq!(row.source_kind, SourceKind::Local);
}

#[test]
fn dry_run_writes_nothing() {
    let scratch = Scratch::new();
    let out = run_chum(&[
        "install",
        &manifest_path_str(&scratch.manifest_path),
        "--root",
        &manifest_path_str(&scratch.chum_root),
        "--dry-run",
    ]);

    assert!(
        out.status.success(),
        "dry-run failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Dry run"), "expected dry-run banner in stdout: {stdout}");
    assert!(stdout.contains("chum-local-test"), "missing manifest name in dry-run: {stdout}");

    // Nothing should have been written to the root.
    assert!(
        !scratch.chum_root.join("state.db").exists(),
        "--dry-run created state.db"
    );
    assert!(
        !scratch.chum_root.join("packages").exists(),
        "--dry-run created packages/"
    );
}

#[test]
fn json_output_is_parseable() {
    let scratch = Scratch::new();
    let out = run_chum(&[
        "install",
        &manifest_path_str(&scratch.manifest_path),
        "--root",
        &manifest_path_str(&scratch.chum_root),
        "--json",
    ]);
    assert!(out.status.success(), "json install failed");

    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["installed"]["name"], "chum-local-test");
    assert_eq!(parsed["installed"]["version"], "0.1.0");
    assert_eq!(parsed["installed"]["source_kind"], "local");
    assert!(
        parsed["installed"]["install_dir"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "install_dir missing in JSON envelope"
    );
}

#[test]
fn duplicate_install_returns_exit_1_with_already_installed() {
    let scratch = Scratch::new();
    let first = run_chum(&[
        "install",
        &manifest_path_str(&scratch.manifest_path),
        "--root",
        &manifest_path_str(&scratch.chum_root),
    ]);
    assert!(first.status.success(), "first install must succeed");

    let second = run_chum(&[
        "install",
        &manifest_path_str(&scratch.manifest_path),
        "--root",
        &manifest_path_str(&scratch.chum_root),
    ]);
    assert!(
        !second.status.success(),
        "second install must fail with exit 1"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("already installed"),
        "expected 'already installed' in stderr: {stderr}"
    );
}

#[test]
fn json_error_envelope_on_bad_manifest() {
    let dir = TempDir::new().unwrap();
    let bad = dir.path().join("not-a-manifest.toml");
    std::fs::write(&bad, b"this is not valid manifest TOML !!!").unwrap();

    let out = run_chum(&[
        "install",
        &manifest_path_str(&bad),
        "--root",
        &manifest_path_str(dir.path()),
        "--json",
    ]);
    assert!(!out.status.success(), "bad manifest must exit non-zero");

    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("error envelope is JSON");
    assert_eq!(parsed["status"], "error");
    assert!(
        parsed["code"].as_str().is_some_and(|c| c.starts_with("manifest_")),
        "expected manifest_* code, got: {}",
        parsed["code"]
    );
}
