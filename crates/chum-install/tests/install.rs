//! Integration tests for chum-install. All tests use a tempdir for the
//! CHUM root and MockFetcher for HTTP — zero network calls.

use std::fmt::Write;

use chum_core::Manifest;
use chum_install::fetcher::test_support::MockFetcher;
use chum_install::{
    install, InstallConfig, InstallError, SourceKind,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Manifest builders — TOML round-tripped through chum_core::parse_str so
// the test fixtures stay in sync with the real schema.
// ---------------------------------------------------------------------------

fn npm_manifest() -> Manifest {
    let toml = r#"
schema_version = "0.1"

[package]
name = "test-pkg"
version = "0.1.0"
description = "test fixture"
license = "MIT"
authors = []

[source]
kind = "npm"
package = "@example/test"
version = "^1.0"

[runtime]
command = "node"

[runtime.transport]
kind = "stdio"
"#;
    chum_core::parse_str(toml).expect("npm manifest parses")
}

fn local_manifest(path: &str) -> Manifest {
    // Literal TOML strings (single-quoted) avoid escape ambiguity for
    // paths like /var/folders/...
    let toml = format!(
        r#"
schema_version = "0.1"

[package]
name = "local-pkg"
version = "0.0.1"
description = "test fixture"
license = "MIT"
authors = []

[source]
kind = "local"
path = '{path}'

[runtime]
command = "echo"

[runtime.transport]
kind = "stdio"
"#
    );
    chum_core::parse_str(&toml).expect("local manifest parses")
}

fn binary_manifest(url: &str, checksum: &str) -> Manifest {
    let toml = format!(
        r#"
schema_version = "0.1"

[package]
name = "binary-pkg"
version = "0.0.1"
description = "test fixture"
license = "MIT"
authors = []

[source]
kind = "binary"
url = "{url}"
checksum_sha256 = "{checksum}"

[runtime]
command = "binary-pkg"

[runtime.transport]
kind = "stdio"
"#
    );
    chum_core::parse_str(&toml).expect("binary manifest parses")
}

fn pypi_manifest() -> Manifest {
    let toml = r#"
schema_version = "0.1"

[package]
name = "pypi-pkg"
version = "0.1.0"
description = "test fixture"
license = "MIT"
authors = []

[source]
kind = "pypi"
package = "test-pkg"
version = "^1.0"

[runtime]
command = "python"

[runtime.transport]
kind = "stdio"
"#;
    chum_core::parse_str(toml).expect("pypi manifest parses")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

fn fake_npm_path() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-npm.sh").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn install_npm_with_mock_subprocess() {
    // Drive install_npm through a fake npm shell script so the test
    // exercises the real subprocess plumbing without needing npm on the
    // host PATH. The fake script creates node_modules/ to mimic the
    // post-condition install_npm relies on.
    let root = TempDir::new().expect("tempdir");
    let manifest = npm_manifest();
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default().with_npm_command(vec![
        "/bin/sh".to_string(),
        fake_npm_path(),
    ]);

    let artifact = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect("install should succeed with fake npm");

    assert_eq!(artifact.source_kind, SourceKind::Npm);
    assert_eq!(artifact.name, "test-pkg");
    assert_eq!(artifact.version, "0.1.0");
    assert!(artifact.install_dir.exists(), "install_dir created");
    assert!(
        artifact.install_dir.join("node_modules").exists(),
        "fake npm should have created node_modules"
    );
}

#[tokio::test]
async fn install_local_symlinks_correctly() {
    let root = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source tempdir");
    tokio::fs::write(source.path().join("README.md"), "hello")
        .await
        .expect("seed source dir");

    let manifest = local_manifest(&source.path().display().to_string());
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default();

    let artifact = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect("local install should succeed");

    assert_eq!(artifact.source_kind, SourceKind::Local);
    assert!(
        artifact.entrypoint.is_symlink(),
        "entrypoint should be a symlink, not a copy"
    );
    let target = tokio::fs::read_link(&artifact.entrypoint)
        .await
        .expect("read_link");
    assert_eq!(target, source.path());
}

#[tokio::test]
async fn install_local_rejects_traversal() {
    let root = TempDir::new().expect("tempdir");
    let manifest = local_manifest("../some/relative/path");
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default();

    let err = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect_err("traversal path should be rejected");
    match err {
        InstallError::PathTraversal(_) => {}
        other => panic!("expected PathTraversal, got {other:?}"),
    }
}

#[tokio::test]
async fn install_binary_verifies_checksum() {
    let root = TempDir::new().expect("tempdir");
    let body = b"hello world".to_vec();
    let checksum = sha256_hex(&body);
    let url = "https://example.invalid/foo";

    let manifest = binary_manifest(url, &checksum);
    let fetcher = MockFetcher::new().with_response(url, body.clone());
    let config = InstallConfig::default();

    let artifact = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect("matching checksum should install");

    assert_eq!(artifact.source_kind, SourceKind::Binary);
    assert!(
        artifact.entrypoint.is_dir(),
        "binary entrypoint = install_dir/bin should be a directory"
    );
    // URL ends in "foo" (no archive ext) → raw byte placement.
    let placed = artifact.entrypoint.join("foo");
    assert!(placed.exists(), "raw bytes placed at bin/foo");
    let on_disk = tokio::fs::read(&placed).await.expect("read placed");
    assert_eq!(on_disk, body, "placed bytes match downloaded bytes");
}

#[tokio::test]
async fn install_binary_rejects_mismatched_checksum() {
    // Security-critical path: an all-zero checksum cannot match any
    // real bytes, so the install must reject hard before writing
    // anything to install_dir/bin.
    let root = TempDir::new().expect("tempdir");
    let body = b"hello world".to_vec();
    let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
    let url = "https://example.invalid/foo";

    let manifest = binary_manifest(url, wrong);
    let fetcher = MockFetcher::new().with_response(url, body);
    let config = InstallConfig::default();

    let err = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect_err("mismatched checksum should reject");

    let (expected, actual) = match err {
        InstallError::ChecksumMismatch { expected, actual } => (expected, actual),
        other => panic!("expected ChecksumMismatch, got {other:?}"),
    };
    assert_eq!(expected, wrong);
    assert_ne!(actual, wrong);
    assert_eq!(actual.len(), 64, "actual digest is 64 hex chars");

    // Belt-and-braces: the bytes should not have been written anywhere
    // under install_dir/bin. (install_dir itself exists because
    // install() create_dir_all-s it before dispatch, by design — the
    // critical invariant is that no payload reached disk.)
    let install_dir = root
        .path()
        .join("packages")
        .join("binary-pkg")
        .join("0.0.1");
    assert!(
        !install_dir.join("bin").exists(),
        "bin/ must not be created on checksum mismatch"
    );
}

#[tokio::test]
async fn install_pypi_returns_unsupported_source() {
    let root = TempDir::new().expect("tempdir");
    let manifest = pypi_manifest();
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default();

    let err = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect_err("pypi should be unsupported in v0.1");
    match err {
        InstallError::UnsupportedSource(kind) => assert_eq!(kind, "pypi"),
        other => panic!("expected UnsupportedSource, got {other:?}"),
    }
}

#[tokio::test]
async fn install_missing_tool_surfaces_specific_error() {
    // Use a binary name that almost certainly does not exist on PATH so
    // tokio::process::Command::output returns ErrorKind::NotFound. The
    // suffix is random-ish to avoid collisions with a real script in
    // someone's dotfiles.
    let missing = "chum-this-binary-does-not-exist-7f3a9";

    let root = TempDir::new().expect("tempdir");
    let manifest = npm_manifest();
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default().with_npm_command(vec![missing.to_string()]);

    let err = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect_err("missing npm binary should surface MissingTool");
    match err {
        InstallError::MissingTool { tool } => assert_eq!(tool, missing),
        other => panic!("expected MissingTool, got {other:?}"),
    }
}

#[tokio::test]
async fn install_writes_chum_manifest_and_logs_dir() {
    // The daemon depends on `<install_dir>/chum-manifest.toml` being
    // present after install so it can re-parse on spawn. The
    // `logs/` directory is where supervisor redirects child
    // stdout/stderr. Both must exist after a successful install of
    // any source kind — this test covers Source::Local; the other
    // source-specific tests above implicitly cover them too because
    // the post-install step runs in the dispatcher.
    let root = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source tempdir");
    tokio::fs::write(source.path().join("SENTINEL"), "x")
        .await
        .expect("seed source");

    let manifest = local_manifest(&source.path().display().to_string());
    let fetcher = MockFetcher::new();
    let config = InstallConfig::default();
    let artifact = install(&manifest, root.path(), &fetcher, &config)
        .await
        .expect("install should succeed");

    let manifest_path = artifact.install_dir.join("chum-manifest.toml");
    assert!(
        manifest_path.is_file(),
        "chum-manifest.toml must exist at {manifest_path:?}"
    );
    let logs_dir = artifact.install_dir.join("logs");
    assert!(logs_dir.is_dir(), "logs dir must exist at {logs_dir:?}");

    // The on-disk manifest should round-trip back into the same
    // Manifest value chum-core parses.
    let on_disk = tokio::fs::read_to_string(&manifest_path)
        .await
        .expect("read manifest");
    let reparsed = chum_core::parse_and_validate(&on_disk)
        .expect("on-disk manifest re-parses cleanly");
    assert_eq!(reparsed.package.name, manifest.package.name);
    assert_eq!(reparsed.package.version, manifest.package.version);
}
