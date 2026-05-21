//! Integration tests for `chum search`.

mod common;

use crate::common::{TestDb, run_chum_with_root};
use tempfile::TempDir;

fn make_manifest(name: &str, description: &str) -> String {
    format!(
        r#"schema_version = "0.1"

[package]
name = "{name}"
version = "0.1.0"
description = "{description}"
license = "MIT"
authors = []

[source]
kind = "local"
path = "/tmp/anything"

[runtime]
command = "echo"

[runtime.transport]
kind = "stdio"
"#
    )
}

fn seed_first_party(dir: &std::path::Path, name: &str, description: &str) {
    std::fs::write(
        dir.join(format!("chum-{name}.toml")),
        make_manifest(name, description),
    )
    .unwrap();
}

fn seed_installed(root: &std::path::Path, name: &str, description: &str) {
    let install_dir = root.join("packages").join(name).join("0.1.0");
    std::fs::create_dir_all(install_dir.join("logs")).unwrap();
    std::fs::write(
        install_dir.join("chum-manifest.toml"),
        make_manifest(name, description),
    )
    .unwrap();
    let registry = chum_registry::Registry::open(root.join("state.db")).unwrap();
    registry
        .insert(&chum_install::InstalledArtifact {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            install_dir,
            entrypoint: std::path::PathBuf::from("/usr/bin/true"),
            source_kind: chum_install::SourceKind::Local,
        })
        .unwrap();
}

#[test]
fn search_unions_first_party_and_installed() {
    let manifests = TempDir::new().unwrap();
    seed_first_party(manifests.path(), "alpha", "first-party alpha description");
    seed_first_party(manifests.path(), "shared", "first-party shared description");

    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_installed(&root, "beta", "installed beta description");
    seed_installed(&root, "shared", "installed shared description");

    let out = run_chum_with_root(
        &root,
        &[
            "search",
            "--manifests-dir",
            manifests.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert!(out.status.success(), "search failed");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = parsed["results"].as_array().unwrap();

    // Three unique names: alpha (available), beta (installed),
    // shared (installed because installed wins).
    let by_name: std::collections::HashMap<&str, &serde_json::Value> = results
        .iter()
        .map(|r| (r["name"].as_str().unwrap(), r))
        .collect();
    assert_eq!(by_name.len(), 3);
    assert_eq!(by_name["alpha"]["status"], "available");
    assert_eq!(by_name["beta"]["status"], "installed");
    assert_eq!(
        by_name["shared"]["status"], "installed",
        "installed must win on collision"
    );
    assert_eq!(
        by_name["shared"]["description"], "installed shared description",
        "installed description must win on collision"
    );
}

#[test]
fn search_query_filters_on_name_and_description() {
    let manifests = TempDir::new().unwrap();
    seed_first_party(manifests.path(), "filesystem", "manage files and directories");
    seed_first_party(manifests.path(), "brave-search", "web search via Brave API");

    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();

    // Match on name.
    let by_name = run_chum_with_root(
        &root,
        &[
            "search",
            "files",
            "--manifests-dir",
            manifests.path().to_str().unwrap(),
            "--json",
        ],
    );
    let parsed: serde_json::Value = serde_json::from_slice(&by_name.stdout).unwrap();
    // "filesystem" contains "files" → matches; "manage files" too.
    let names: Vec<&str> = parsed["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"filesystem"));
    assert!(!names.contains(&"brave-search"));

    // Match on description.
    let by_desc = run_chum_with_root(
        &root,
        &[
            "search",
            "Brave",
            "--manifests-dir",
            manifests.path().to_str().unwrap(),
            "--json",
        ],
    );
    let parsed2: serde_json::Value = serde_json::from_slice(&by_desc.stdout).unwrap();
    let names2: Vec<&str> = parsed2["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(names2.contains(&"brave-search"));
    assert!(!names2.contains(&"filesystem"));
}

#[test]
fn search_filters_installed_only_and_available_only() {
    let manifests = TempDir::new().unwrap();
    seed_first_party(manifests.path(), "available-pkg", "available");

    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_installed(&root, "installed-pkg", "installed");

    let inst = run_chum_with_root(
        &root,
        &[
            "search",
            "--installed-only",
            "--manifests-dir",
            manifests.path().to_str().unwrap(),
            "--json",
        ],
    );
    let p: serde_json::Value = serde_json::from_slice(&inst.stdout).unwrap();
    let names: Vec<&str> = p["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["installed-pkg"]);

    let avail = run_chum_with_root(
        &root,
        &[
            "search",
            "--available-only",
            "--manifests-dir",
            manifests.path().to_str().unwrap(),
            "--json",
        ],
    );
    let p2: serde_json::Value = serde_json::from_slice(&avail.stdout).unwrap();
    let names2: Vec<&str> = p2["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names2, vec!["available-pkg"]);
}

#[test]
fn search_with_no_manifests_dir_returns_installed_only() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    seed_installed(&root, "lonely", "only one");

    let out = run_chum_with_root(
        &root,
        &[
            "search",
            "--manifests-dir",
            "/does/not/exist",
            "--json",
        ],
    );
    assert!(out.status.success(), "missing manifests-dir must not error");
    let p: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = p["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "lonely");
}

#[test]
fn search_empty_everything_succeeds_with_empty_results() {
    let db = TestDb::new();
    let root = db._dir.path().to_path_buf();
    let out = run_chum_with_root(
        &root,
        &["search", "--manifests-dir", "/does/not/exist", "--json"],
    );
    assert!(out.status.success());
    let p: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(p["status"], "ok");
    assert!(p["results"].as_array().unwrap().is_empty());
}
