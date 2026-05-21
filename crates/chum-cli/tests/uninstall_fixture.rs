//! Integration tests for `chum uninstall`.
//!
//! These tests exercise the registry → filesystem → registry cycle:
//! install (via the cli), confirm the row + dir, uninstall (via the
//! cli), confirm both are gone. ZERO network — Source::Local only.

mod common;

use std::path::PathBuf;

use chum_install::{InstalledArtifact, SourceKind};
use chum_registry::Registry;

use crate::common::{Scratch, path_str, run_chum};

/// Install via the cli, then return the install_dir path that should
/// now exist under the scratch's CHUM root.
fn install_via_cli(scratch: &Scratch) -> PathBuf {
    let out = run_chum(&[
        "install",
        &path_str(&scratch.manifest_path),
        "--root",
        &path_str(&scratch.chum_root),
    ]);
    assert!(
        out.status.success(),
        "install failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    scratch
        .chum_root
        .join("packages")
        .join("chum-local-test")
        .join("0.1.0")
}

#[test]
fn install_then_uninstall_then_list_shows_empty() {
    let scratch = Scratch::new();
    let install_dir = install_via_cli(&scratch);
    assert!(install_dir.exists(), "install_dir must exist after install");

    // list shows the row.
    let mid = run_chum(&[
        "list",
        "--root",
        &path_str(&scratch.chum_root),
        "--json",
    ]);
    assert!(mid.status.success());
    let mid_json: serde_json::Value = serde_json::from_slice(&mid.stdout).unwrap();
    assert_eq!(mid_json["packages"].as_array().unwrap().len(), 1);

    // uninstall removes it.
    let un = run_chum(&[
        "uninstall",
        "chum-local-test",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
    ]);
    assert!(
        un.status.success(),
        "uninstall failed: stderr={}",
        String::from_utf8_lossy(&un.stderr),
    );
    let un_stdout = String::from_utf8_lossy(&un.stdout);
    assert!(
        un_stdout.contains("Uninstalled chum-local-test 0.1.0"),
        "missing confirmation: {un_stdout}"
    );

    // Filesystem: install_dir is gone.
    assert!(
        !install_dir.exists(),
        "install_dir should be removed after uninstall"
    );

    // Registry: row is gone.
    let final_list = run_chum(&[
        "list",
        "--root",
        &path_str(&scratch.chum_root),
        "--json",
    ]);
    assert!(final_list.status.success());
    let final_json: serde_json::Value = serde_json::from_slice(&final_list.stdout).unwrap();
    assert_eq!(final_json["packages"], serde_json::json!([]));
}

#[test]
fn uninstall_nonexistent_returns_not_installed() {
    let scratch = Scratch::new();
    // Don't install anything.
    let out = run_chum(&[
        "uninstall",
        "ghost-package",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
        "--json",
    ]);
    assert!(!out.status.success(), "uninstalling nothing must exit 1");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["code"], "not_installed");
}

#[test]
fn uninstall_ambiguous_requires_version() {
    let scratch = Scratch::new();
    install_via_cli(&scratch);

    // Inject a second version directly into the registry so the cli
    // sees an ambiguous match. We don't need the second install_dir
    // to actually exist — the ambiguity check happens before the
    // filesystem touch.
    {
        let registry =
            Registry::open(scratch.chum_root.join("state.db")).expect("reopen registry");
        let second_dir = scratch
            .chum_root
            .join("packages")
            .join("chum-local-test")
            .join("0.2.0");
        registry
            .insert(&InstalledArtifact {
                name: "chum-local-test".to_string(),
                version: "0.2.0".to_string(),
                install_dir: second_dir.clone(),
                entrypoint: second_dir.join("local-src"),
                source_kind: SourceKind::Local,
            })
            .expect("insert second row");
    }

    // No --version passed → ambiguity error.
    let out = run_chum(&[
        "uninstall",
        "chum-local-test",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
        "--json",
    ]);
    assert!(!out.status.success(), "ambiguity must exit 1");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["code"], "ambiguous_version");
    let msg = parsed["message"].as_str().unwrap();
    assert!(msg.contains("0.1.0"), "msg should list versions: {msg}");
    assert!(msg.contains("0.2.0"), "msg should list versions: {msg}");

    // Explicit version resolves the ambiguity.
    let resolved = run_chum(&[
        "uninstall",
        "chum-local-test",
        "0.2.0",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
    ]);
    assert!(
        resolved.status.success(),
        "explicit-version uninstall must succeed: stderr={}",
        String::from_utf8_lossy(&resolved.stderr),
    );
}

#[test]
fn uninstall_with_version_flag_works() {
    let scratch = Scratch::new();
    install_via_cli(&scratch);

    let out = run_chum(&[
        "uninstall",
        "chum-local-test",
        "--version",
        "0.1.0",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
    ]);
    assert!(
        out.status.success(),
        "--version flag form must succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn keep_files_leaves_disk_removes_row() {
    let scratch = Scratch::new();
    let install_dir = install_via_cli(&scratch);
    let symlink = install_dir.join("local-src");
    assert!(symlink.exists() || symlink.is_symlink());

    let out = run_chum(&[
        "uninstall",
        "chum-local-test",
        "--root",
        &path_str(&scratch.chum_root),
        "--keep-files",
        "--force",
    ]);
    assert!(
        out.status.success(),
        "--keep-files uninstall must succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("files retained"),
        "missing retention notice: {stdout}"
    );

    // Filesystem: install_dir still present.
    assert!(
        install_dir.is_dir(),
        "--keep-files must leave install_dir intact"
    );

    // Registry: row gone.
    let listed = run_chum(&[
        "list",
        "--root",
        &path_str(&scratch.chum_root),
        "--json",
    ]);
    let parsed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(
        parsed["packages"],
        serde_json::json!([]),
        "registry row should be gone after --keep-files uninstall"
    );
}

#[test]
fn uninstall_json_envelope_shape() {
    let scratch = Scratch::new();
    install_via_cli(&scratch);

    let out = run_chum(&[
        "uninstall",
        "chum-local-test",
        "--root",
        &path_str(&scratch.chum_root),
        "--force",
        "--json",
    ]);
    assert!(out.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("JSON parse");
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["uninstalled"]["name"], "chum-local-test");
    assert_eq!(parsed["uninstalled"]["version"], "0.1.0");
    assert_eq!(parsed["uninstalled"]["keep_files"], false);
}
