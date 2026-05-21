//! Integration tests for `chum permit / revoke / permissions`.

mod common;

use std::process::Output;

use crate::common::{TestDb, run_chum_with_root};

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

/// Build a fixture by writing a registry row + manifest directly,
/// without going through `chum install`. The manifest TOML is
/// supplied verbatim so each test can declare exactly the
/// permissions it cares about.
fn seed_install(root: &std::path::Path, name: &str, version: &str, manifest_body: &str) {
    let install_dir = root.join("packages").join(name).join(version);
    std::fs::create_dir_all(install_dir.join("logs")).unwrap();
    std::fs::write(install_dir.join("chum-manifest.toml"), manifest_body).unwrap();

    let registry = chum_registry::Registry::open(root.join("state.db")).unwrap();
    registry
        .insert(&chum_install::InstalledArtifact {
            name: name.to_string(),
            version: version.to_string(),
            install_dir,
            entrypoint: std::path::PathBuf::from("/usr/bin/true"),
            source_kind: chum_install::SourceKind::Local,
        })
        .unwrap();
}

fn permit(root: &std::path::Path, name: &str, grants: &[&str]) -> Output {
    let mut args = vec!["permit".to_string(), name.to_string()];
    for g in grants {
        args.push("--grant".to_string());
        args.push(g.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_chum_with_root(root, &arg_refs)
}

fn revoke(root: &std::path::Path, name: &str, grant: &str) -> Output {
    run_chum_with_root(root, &["revoke", name, "--grant", grant])
}

fn permissions(root: &std::path::Path, name: &str) -> Output {
    run_chum_with_root(root, &["permissions", name, "--json"])
}

fn manifest_with(permissions_block: &str) -> String {
    format!(
        r#"schema_version = "0.1"

[package]
name = "broker-cli-test"
version = "0.1.0"
description = "broker cli test fixture"
license = "MIT"
authors = []

[source]
kind = "local"
path = "/tmp/whatever"

[runtime]
command = "echo"

[runtime.transport]
kind = "stdio"

{permissions_block}
"#
    )
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[test]
fn permit_adds_a_grant_and_permissions_reflects_it() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with(
            r#"[permissions.env]
read = ["BRAVE_API_KEY"]"#,
        ),
    );

    let out = permit(
        &root,
        "broker-cli-test",
        &["env.read=BRAVE_API_KEY"],
    );
    assert!(
        out.status.success(),
        "permit failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let perms = permissions(&root, "broker-cli-test");
    assert!(perms.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&perms.stdout).unwrap();
    assert_eq!(parsed["status"], "ok");
    let granted = parsed["permissions"]["granted"].as_array().unwrap();
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0]["kind"], "env.read");
    assert_eq!(granted[0]["value"], "BRAVE_API_KEY");
    let missing = parsed["permissions"]["missing"].as_array().unwrap();
    assert!(missing.is_empty(), "missing should be empty after permit");
}

#[test]
fn permit_repeats_are_no_op() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with("[permissions]"),
    );

    let first = permit(&root, "broker-cli-test", &["env.read=FOO"]);
    assert!(first.status.success());
    let second = permit(&root, "broker-cli-test", &["env.read=FOO"]);
    assert!(
        second.status.success(),
        "repeat permit must be a no-op (idempotent), not an error"
    );
}

#[test]
fn permit_rejects_unknown_kind() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with("[permissions]"),
    );

    let out = permit(&root, "broker-cli-test", &["not.a.kind=value"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a known permission"),
        "expected unknown_permission error, got: {stderr}"
    );
}

#[test]
fn revoke_removes_and_then_permissions_shows_missing() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with(
            r#"[permissions.env]
read = ["FOO"]"#,
        ),
    );

    let _ = permit(&root, "broker-cli-test", &["env.read=FOO"]);
    let r = revoke(&root, "broker-cli-test", "env.read=FOO");
    assert!(
        r.status.success(),
        "revoke failed: stderr={}",
        String::from_utf8_lossy(&r.stderr),
    );

    let perms = permissions(&root, "broker-cli-test");
    let parsed: serde_json::Value = serde_json::from_slice(&perms.stdout).unwrap();
    let missing = parsed["permissions"]["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["kind"], "env.read");
    assert_eq!(missing[0]["value"], "FOO");
}

#[test]
fn revoke_missing_returns_grant_not_found() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with("[permissions]"),
    );

    let out = revoke(&root, "broker-cli-test", "env.read=NEVER_GRANTED");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no grant") && stderr.contains("env.read=NEVER_GRANTED"),
        "expected grant_not_found wording, got: {stderr}"
    );
}

#[test]
fn permissions_diff_shape_with_partial_grants() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_install(
        &root,
        "broker-cli-test",
        "0.1.0",
        &manifest_with(
            r#"[permissions.filesystem]
read = ["/Users/x/Documents"]
[permissions.env]
read = ["FOO", "BAR"]"#,
        ),
    );

    let _ = permit(&root, "broker-cli-test", &["env.read=FOO"]);

    let perms = permissions(&root, "broker-cli-test");
    let parsed: serde_json::Value = serde_json::from_slice(&perms.stdout).unwrap();
    let declared = parsed["permissions"]["declared"].as_array().unwrap();
    assert_eq!(declared.len(), 3);
    let granted = parsed["permissions"]["granted"].as_array().unwrap();
    assert_eq!(granted.len(), 1);
    let missing = parsed["permissions"]["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 2);
}
