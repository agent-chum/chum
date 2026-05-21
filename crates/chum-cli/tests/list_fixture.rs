//! Integration tests for `chum list`.

mod common;

use crate::common::{Scratch, path_str, run_chum};

#[test]
fn list_empty_returns_no_packages() {
    let scratch = Scratch::new();
    // Don't install anything — list must still exit 0.
    let out = run_chum(&["list", "--root", &path_str(&scratch.chum_root)]);

    assert!(
        out.status.success(),
        "list on empty root failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("No packages installed"),
        "expected empty-state banner, got: {stdout}"
    );

    // Critically: chum list on an empty root must not pollute the
    // filesystem with an empty state.db.
    assert!(
        !scratch.chum_root.join("state.db").exists(),
        "chum list on empty root must not create state.db"
    );
}

#[test]
fn list_empty_json_is_parseable_empty_array() {
    let scratch = Scratch::new();
    let out = run_chum(&["list", "--root", &path_str(&scratch.chum_root), "--json"]);

    assert!(out.status.success(), "empty --json list failed");
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["packages"], serde_json::json!([]));
}

#[test]
fn install_then_list_shows_row() {
    let scratch = Scratch::new();

    let install = run_chum(&[
        "install",
        &path_str(&scratch.manifest_path),
        "--root",
        &path_str(&scratch.chum_root),
    ]);
    assert!(install.status.success(), "install must succeed first");

    let list = run_chum(&["list", "--root", &path_str(&scratch.chum_root)]);
    assert!(list.status.success(), "list after install must succeed");
    let stdout = String::from_utf8_lossy(&list.stdout);

    assert!(stdout.contains("NAME"), "expected table header, got: {stdout}");
    assert!(stdout.contains("chum-local-test"), "row missing: {stdout}");
    assert!(stdout.contains("0.1.0"), "version missing: {stdout}");
    assert!(stdout.contains("local"), "kind missing: {stdout}");
    // PATH column is relative to root: packages/<name>/<version>.
    assert!(
        stdout.contains("packages/chum-local-test/0.1.0"),
        "relative PATH missing: {stdout}"
    );
}

#[test]
fn install_then_list_json_envelope_shape() {
    let scratch = Scratch::new();
    let install = run_chum(&[
        "install",
        &path_str(&scratch.manifest_path),
        "--root",
        &path_str(&scratch.chum_root),
    ]);
    assert!(install.status.success());

    let list = run_chum(&["list", "--root", &path_str(&scratch.chum_root), "--json"]);
    assert!(list.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("JSON parse");
    assert_eq!(parsed["status"], "ok");
    let pkgs = parsed["packages"].as_array().expect("packages array");
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0]["name"], "chum-local-test");
    assert_eq!(pkgs[0]["version"], "0.1.0");
    assert_eq!(pkgs[0]["source_kind"], "local");
    // No `id` field in the envelope (registry internal).
    assert!(pkgs[0].get("id").is_none(), "id should not appear in JSON");
}

#[test]
fn list_prefix_filter_excludes_non_matches() {
    let scratch = Scratch::new();
    let install = run_chum(&[
        "install",
        &path_str(&scratch.manifest_path),
        "--root",
        &path_str(&scratch.chum_root),
    ]);
    assert!(install.status.success());

    // Prefix that matches — row appears.
    let hit = run_chum(&[
        "list",
        "chum-",
        "--root",
        &path_str(&scratch.chum_root),
        "--json",
    ]);
    assert!(hit.status.success());
    let hit_parsed: serde_json::Value = serde_json::from_slice(&hit.stdout).unwrap();
    assert_eq!(hit_parsed["packages"].as_array().unwrap().len(), 1);

    // Prefix that misses — empty list, still exit 0.
    let miss = run_chum(&[
        "list",
        "nonexistent-",
        "--root",
        &path_str(&scratch.chum_root),
        "--json",
    ]);
    assert!(miss.status.success());
    let miss_parsed: serde_json::Value = serde_json::from_slice(&miss.stdout).unwrap();
    assert_eq!(miss_parsed["packages"], serde_json::json!([]));
}
