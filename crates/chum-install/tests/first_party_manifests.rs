//! Smoke test for the first-party manifests shipped in `manifests/`.
//!
//! Each file under `manifests/` must:
//! 1. Parse + validate cleanly via `chum_core::parse_and_validate`.
//! 2. Declare a `Source::Npm` source pointing at an
//!    `@modelcontextprotocol/server-*` package.
//! 3. Use a `stdio` transport (every official server is stdio-based
//!    today).
//!
//! Real install testing (`npx -y …`) needs npm and network — that's a
//! manual smoke step, not a CI dependency. This test is structural only.

use std::path::PathBuf;

use chum_core::manifest::{Source, Transport};

/// Absolute path to the workspace's `manifests/` directory.
fn manifests_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/chum-install/`. The `manifests/`
    // directory lives two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("manifests")
        .canonicalize()
        .expect("manifests/ should exist at the workspace root")
}

#[test]
fn first_party_manifests_parse_and_have_npm_source() {
    let dir = manifests_dir();
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("read_dir manifests/")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("toml"))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();

    assert!(
        entries.len() >= 8,
        "expected at least 8 first-party manifests in {dir:?}, found {}",
        entries.len()
    );

    for path in &entries {
        let body = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let manifest = chum_core::parse_and_validate(&body)
            .unwrap_or_else(|e| panic!("parse {path:?}: {e}"));

        // Filename convention: `chum-<name>.toml` with the same `<name>`
        // showing up in `[package].name`. The convention is documented in
        // CHUM_SCOPE; this assertion locks it in.
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let expected_name = stem
            .strip_prefix("chum-")
            .unwrap_or_else(|| panic!("{path:?}: filename must start with `chum-`"));
        assert_eq!(
            manifest.package.name, expected_name,
            "{path:?}: package.name does not match filename"
        );

        match &manifest.source {
            Source::Npm { package, version } => {
                assert!(
                    package.starts_with("@modelcontextprotocol/server-"),
                    "{path:?}: source.package should be an official @modelcontextprotocol/server-* (got {package})"
                );
                assert!(
                    !version.is_empty(),
                    "{path:?}: source.version must be non-empty"
                );
            }
            other => panic!(
                "{path:?}: v0.1 first-party manifests must use Source::Npm, got {other:?}"
            ),
        }

        match &manifest.runtime.transport {
            Transport::Stdio => {}
            other => panic!(
                "{path:?}: v0.1 first-party manifests must use stdio transport, got {other:?}"
            ),
        }

        // Sanity: command must be `npx` (the convention all 10 manifests
        // follow).
        assert_eq!(
            manifest.runtime.command, "npx",
            "{path:?}: command should be `npx` for npm-source manifests"
        );

        // `[capabilities].tools` should be non-empty — every server we
        // ship advertises at least one tool. Catches accidental empties
        // from copy-paste.
        assert!(
            !manifest.capabilities.tools.is_empty(),
            "{path:?}: capabilities.tools must declare at least one tool"
        );
    }
}
