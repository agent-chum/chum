//! Integration tests for the v0.1 manifest schema.

use chum_core::manifest::{Health, Lifecycle, RestartPolicy, Source, Transport};
use chum_core::{ManifestError, parse_and_validate, parse_str, validate};

const FILESYSTEM: &str = include_str!("fixtures/chum-filesystem.toml");
const BRAVE_SEARCH: &str = include_str!("fixtures/chum-brave-search.toml");
const SQLITE_BINARY: &str = include_str!("fixtures/chum-sqlite.toml");
const LOCAL_DEV: &str = include_str!("fixtures/chum-local-dev.toml");
const POSTGRES_HTTP: &str = include_str!("fixtures/chum-postgres-remote.toml");

const INVALID_UNKNOWN_SCHEMA: &str = include_str!("fixtures/invalid-unknown-schema.toml");
const INVALID_BINARY_NO_CHECKSUM: &str = include_str!("fixtures/invalid-binary-no-checksum.toml");
const INVALID_BAD_NAME: &str = include_str!("fixtures/invalid-bad-name.toml");
const INVALID_BIND_ZERO: &str = include_str!("fixtures/invalid-bind-zero.toml");

#[test]
fn parse_filesystem_npm_ok() {
    let manifest = parse_str(FILESYSTEM).expect("filesystem fixture should parse");
    assert_eq!(manifest.schema_version, "0.1");
    assert_eq!(manifest.package.name, "filesystem");
    assert_eq!(manifest.package.version, "0.1.0");
    assert!(manifest
        .package
        .tags
        .iter()
        .any(|t| t == "official"));
    match manifest.source {
        Source::Npm { ref package, ref version } => {
            assert_eq!(package, "@modelcontextprotocol/server-filesystem");
            assert_eq!(version, "^0.1");
        }
        _ => panic!("expected npm source"),
    }
    assert_eq!(manifest.runtime.command, "npx");
    assert_eq!(manifest.runtime.args.len(), 3);
    assert!(matches!(manifest.runtime.transport, Transport::Stdio));
    assert!(matches!(manifest.health, Health::Process));
    assert!(manifest.capabilities.tools.iter().any(|t| t == "read_file"));
}

#[test]
fn parse_brave_search_npm_env_ok() {
    let manifest = parse_str(BRAVE_SEARCH).expect("brave-search fixture should parse");
    assert_eq!(manifest.package.name, "brave-search");
    assert_eq!(
        manifest.runtime.env.get("BRAVE_API_KEY").map(String::as_str),
        Some("${BRAVE_API_KEY}")
    );
}

#[test]
fn parse_sqlite_binary_ok() {
    let manifest = parse_str(SQLITE_BINARY).expect("sqlite fixture should parse");
    match manifest.source {
        Source::Binary {
            ref url,
            ref checksum_sha256,
            ref target_triple,
        } => {
            assert!(url.starts_with("https://"));
            assert_eq!(checksum_sha256.len(), 64);
            assert_eq!(target_triple.as_deref(), Some("aarch64-apple-darwin"));
        }
        _ => panic!("expected binary source"),
    }
}

#[test]
fn parse_local_path_ok() {
    let manifest = parse_str(LOCAL_DEV).expect("local-dev fixture should parse");
    match manifest.source {
        Source::Local { ref path } => assert!(path.starts_with('/')),
        _ => panic!("expected local source"),
    }
    assert!(matches!(manifest.lifecycle.restart, RestartPolicy::Never));
}

#[test]
fn parse_postgres_http_ok() {
    let manifest = parse_str(POSTGRES_HTTP).expect("postgres fixture should parse");
    match manifest.runtime.transport {
        Transport::Http { port, ref bind, ref path } => {
            assert_eq!(port, 8080);
            assert_eq!(bind, "127.0.0.1");
            assert_eq!(path.as_deref(), Some("/mcp"));
        }
        _ => panic!("expected http transport"),
    }
    match manifest.health {
        Health::Http {
            ref url,
            expect_status,
            ..
        } => {
            assert!(url.starts_with("http://127.0.0.1"));
            assert_eq!(expect_status, 200);
        }
        _ => panic!("expected http health"),
    }
    assert!(matches!(manifest.lifecycle.restart, RestartPolicy::Always));
}

#[test]
fn roundtrip_filesystem_preserves_structure() {
    let original = parse_str(FILESYSTEM).expect("parse");
    let serialised = toml::to_string(&original).expect("serialise");
    let reparsed = parse_str(&serialised).expect("re-parse");
    assert_eq!(original, reparsed, "round-trip lost or transformed data\n--- serialised ---\n{serialised}");
}

#[test]
fn roundtrip_sqlite_binary_preserves_structure() {
    let original = parse_str(SQLITE_BINARY).expect("parse");
    let serialised = toml::to_string(&original).expect("serialise");
    let reparsed = parse_str(&serialised).expect("re-parse");
    assert_eq!(original, reparsed);
}

#[test]
fn roundtrip_postgres_http_preserves_structure() {
    let original = parse_str(POSTGRES_HTTP).expect("parse");
    let serialised = toml::to_string(&original).expect("serialise");
    let reparsed = parse_str(&serialised).expect("re-parse");
    assert_eq!(original, reparsed);
}

#[test]
fn validate_happy_filesystem() {
    let manifest = parse_str(FILESYSTEM).expect("parse");
    validate(&manifest).expect("filesystem should validate");
}

#[test]
fn parse_and_validate_filesystem_ok() {
    let manifest = parse_and_validate(FILESYSTEM).expect("parse_and_validate");
    assert_eq!(manifest.package.name, "filesystem");
}

#[test]
fn permissions_parse_typed_round_trip() {
    // The filesystem fixture carries a typed `[permissions]` block.
    // v0.1 chum-core decodes it into the `Permissions` struct;
    // round-tripping through toml::to_string must preserve the
    // declared shape.
    let manifest = parse_str(FILESYSTEM).expect("parse");
    let perms = &manifest.permissions;
    assert!(!perms.is_empty());
    assert!(perms.filesystem.read.iter().any(|p| p.contains("Documents")));
    assert!(perms.filesystem.write.iter().any(|p| p.contains("Documents")));
    assert!(perms.env.read.iter().any(|v| v == "HOME"));

    let serialised = toml::to_string(&manifest).expect("serialise");
    let reparsed = parse_str(&serialised).expect("re-parse");
    assert_eq!(manifest.permissions, reparsed.permissions);
}

#[test]
fn restart_policy_serialises_to_kebab_case() {
    fn ser(policy: RestartPolicy) -> String {
        let l = Lifecycle {
            restart: policy,
            startup_timeout_sec: 10,
            shutdown_grace_sec: 5,
        };
        toml::to_string(&l).expect("serialise")
    }
    assert!(ser(RestartPolicy::Always).contains("restart = \"always\""));
    assert!(ser(RestartPolicy::OnFailure).contains("restart = \"on-failure\""));
    assert!(ser(RestartPolicy::Never).contains("restart = \"never\""));
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

#[test]
fn parse_unknown_schema_version_rejected() {
    match parse_str(INVALID_UNKNOWN_SCHEMA) {
        Err(ManifestError::UnsupportedSchemaVersion(v)) => assert_eq!(v, "9.9"),
        other => panic!("expected UnsupportedSchemaVersion, got {other:?}"),
    }
}

#[test]
fn parse_missing_schema_version_rejected() {
    // schema_version is mandatory; its absence is a distinct error from a
    // present-but-unsupported version. Older chum builds reading newer
    // manifests get UnsupportedSchemaVersion; brand-new manifests with no
    // schema_version at all get MissingSchemaVersion.
    let toml = r#"
[package]
name = "no-version"
version = "0.0.1"
description = "no schema_version declared"
license = "MIT"
authors = []
"#;
    match parse_str(toml) {
        Err(ManifestError::MissingSchemaVersion) => {}
        other => panic!("expected MissingSchemaVersion, got {other:?}"),
    }
}

#[test]
fn parse_binary_missing_checksum_fails_with_deser_error() {
    // A binary source missing `checksum_sha256` surfaces as a serde
    // deserialisation error in v0.1 (ManifestError::Toml) rather than as a
    // dedicated semantic variant. See `error.rs` for the rationale and the
    // v0.2 revisit plan: cleaning this up needs custom Deserialize, and we
    // chose to keep schema work scoped to v0.1.
    match parse_str(INVALID_BINARY_NO_CHECKSUM) {
        Err(ManifestError::Toml(_)) => {}
        other => panic!("expected ManifestError::Toml, got {other:?}"),
    }
}

#[test]
fn validate_invalid_name_rejected() {
    // A bad name parses cleanly — names are strings, and serde does not
    // know the format constraint. The validate pass catches it.
    let manifest = parse_str(INVALID_BAD_NAME).expect("structural parse succeeds");
    match validate(&manifest) {
        Err(ManifestError::InvalidName(n)) => assert_eq!(n, "BadName!"),
        other => panic!("expected InvalidName, got {other:?}"),
    }
}

#[test]
fn validate_invalid_checksum_rejected() {
    // A checksum that is not 64 hex characters is a semantic violation. It
    // passes structural parse (it is a String) and trips validate.
    let toml = r#"
schema_version = "0.1"

[package]
name = "bad-checksum"
version = "0.1.0"
description = "Binary source with malformed checksum."
license = "MIT"
authors = ["test"]

[source]
kind = "binary"
url = "https://example.com/binary"
checksum_sha256 = "not-a-valid-hex-digest"

[runtime]
command = "x"

[runtime.transport]
kind = "stdio"
"#;
    let manifest = parse_str(toml).expect("structural parse succeeds");
    match validate(&manifest) {
        Err(ManifestError::InvalidChecksum(c)) => assert_eq!(c, "not-a-valid-hex-digest"),
        other => panic!("expected InvalidChecksum, got {other:?}"),
    }
}

#[test]
fn validate_zero_bind_address_rejected() {
    // The 127.0.0.1 default is applied at deserialise time; this fixture
    // explicitly sets bind = "0.0.0.0", which v0.1 chum-core rejects to
    // enforce the local-first invariant.
    let manifest = parse_str(INVALID_BIND_ZERO).expect("structural parse succeeds");
    match validate(&manifest) {
        Err(ManifestError::InvalidBindAddress(b)) => assert_eq!(b, "0.0.0.0"),
        other => panic!("expected InvalidBindAddress, got {other:?}"),
    }
}

#[test]
fn parse_and_validate_propagates_validation_error() {
    // parse_and_validate must surface validate-time errors, not silently
    // swallow them — otherwise the convenience entry point gives a weaker
    // guarantee than its name implies.
    match parse_and_validate(INVALID_BAD_NAME) {
        Err(ManifestError::InvalidName(n)) => assert_eq!(n, "BadName!"),
        other => panic!("expected InvalidName via parse_and_validate, got {other:?}"),
    }
}
