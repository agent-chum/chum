//! Integration tests for `chum env list / set / unset`.

mod common;

use std::path::Path;

use crate::common::{TestDb, run_chum_with_root};

fn manifest_with_env(env_block: &str) -> String {
    format!(
        r#"schema_version = "0.1"

[package]
name = "env-cli-test"
version = "0.1.0"
description = "env cli test fixture"
license = "MIT"
authors = []

[source]
kind = "local"
path = "/tmp/whatever"

[runtime]
command = "echo"

[runtime.transport]
kind = "stdio"

[permissions.env]
read = ["DECLARED_KEY"]

{env_block}
"#
    )
}

fn seed_install(root: &Path, manifest_body: &str) {
    let install_dir = root.join("packages").join("env-cli-test").join("0.1.0");
    std::fs::create_dir_all(install_dir.join("logs")).unwrap();
    std::fs::write(install_dir.join("chum-manifest.toml"), manifest_body).unwrap();
    let registry = chum_registry::Registry::open(root.join("state.db")).unwrap();
    registry
        .insert(&chum_install::InstalledArtifact {
            name: "env-cli-test".to_string(),
            version: "0.1.0".to_string(),
            install_dir,
            entrypoint: std::path::PathBuf::from("/usr/bin/true"),
            source_kind: chum_install::SourceKind::Local,
        })
        .unwrap();
}

#[test]
fn env_list_shows_declared_unset_and_set_keys() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        &manifest_with_env(
            r#"[runtime.env]
EXTRA_KEY = "hello""#,
        ),
    );

    let out = run_chum_with_root(&root, &["env", "list", "env-cli-test", "--json"]);
    assert!(
        out.status.success(),
        "env list failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let entries = parsed["env"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let by_key: std::collections::HashMap<&str, &str> = entries
        .iter()
        .map(|e| {
            (
                e["key"].as_str().unwrap(),
                e["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(by_key["DECLARED_KEY"], "unset");
    assert_eq!(by_key["EXTRA_KEY"], "set");
}

#[test]
fn env_list_never_echoes_values() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        &manifest_with_env(
            r#"[runtime.env]
SECRET = "shhh-do-not-print-me""#,
        ),
    );

    // Plain text.
    let plain = run_chum_with_root(&root, &["env", "list", "env-cli-test"]);
    assert!(plain.status.success());
    let stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(stdout.contains("SECRET"), "key should appear");
    assert!(
        !stdout.contains("shhh-do-not-print-me"),
        "value must NEVER appear in human output: {stdout}"
    );

    // JSON.
    let json = run_chum_with_root(&root, &["env", "list", "env-cli-test", "--json"]);
    assert!(json.status.success());
    let json_stdout = String::from_utf8_lossy(&json.stdout);
    assert!(json_stdout.contains("SECRET"), "key should appear in JSON");
    assert!(
        !json_stdout.contains("shhh-do-not-print-me"),
        "value must NEVER appear in JSON output: {json_stdout}"
    );
}

#[test]
fn env_set_writes_value_to_manifest() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(&root, &manifest_with_env(""));

    let out = run_chum_with_root(
        &root,
        &["env", "set", "env-cli-test", "MY_KEY=my-value"],
    );
    assert!(
        out.status.success(),
        "env set failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    // Confirm via re-parse of the on-disk manifest.
    let manifest_path = root
        .join("packages/env-cli-test/0.1.0/chum-manifest.toml");
    let text = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = chum_core::parse_and_validate(&text).unwrap();
    assert_eq!(manifest.runtime.env.get("MY_KEY").unwrap(), "my-value");

    // Confirmation does NOT echo the value.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MY_KEY"));
    assert!(
        !stdout.contains("my-value"),
        "value must never appear in set confirmation: {stdout}"
    );
}

#[test]
fn env_set_handles_value_with_equals_signs() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(&root, &manifest_with_env(""));

    let out = run_chum_with_root(
        &root,
        &["env", "set", "env-cli-test", "URL=https://api.example.com/v1?a=b"],
    );
    assert!(out.status.success());

    let manifest_path = root
        .join("packages/env-cli-test/0.1.0/chum-manifest.toml");
    let text = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = chum_core::parse_and_validate(&text).unwrap();
    assert_eq!(
        manifest.runtime.env.get("URL").unwrap(),
        "https://api.example.com/v1?a=b"
    );
}

#[test]
fn env_set_rejects_invalid_keys() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(&root, &manifest_with_env(""));

    for bad in &["123ABC=val", "WITH-DASH=val", "WITH SPACE=val", "=val"] {
        let out = run_chum_with_root(&root, &["env", "set", "env-cli-test", bad]);
        assert!(!out.status.success(), "set must reject '{bad}'");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("env key") || stderr.contains("invalid"),
            "expected env_key_invalid wording for '{bad}', got: {stderr}",
        );
    }
}

#[test]
fn env_unset_removes_key_and_is_idempotent() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        &manifest_with_env(
            r#"[runtime.env]
GOING_AWAY = "byebye""#,
        ),
    );

    // First unset removes the key.
    let first = run_chum_with_root(
        &root,
        &["env", "unset", "env-cli-test", "GOING_AWAY"],
    );
    assert!(first.status.success(), "first unset must succeed");

    let manifest_path = root
        .join("packages/env-cli-test/0.1.0/chum-manifest.toml");
    let manifest =
        chum_core::parse_and_validate(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert!(!manifest.runtime.env.contains_key("GOING_AWAY"));

    // Second unset is idempotent.
    let second = run_chum_with_root(
        &root,
        &["env", "unset", "env-cli-test", "GOING_AWAY"],
    );
    assert!(
        second.status.success(),
        "second unset must be idempotent (success, not error)"
    );
}
