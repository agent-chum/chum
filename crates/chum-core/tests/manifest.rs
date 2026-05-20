//! Integration tests for the v0.1 manifest schema.

use chum_core::manifest::{Health, Lifecycle, RestartPolicy, Source, Transport};
use chum_core::{parse_and_validate, parse_str, validate};

const FILESYSTEM: &str = include_str!("fixtures/chum-filesystem.toml");
const BRAVE_SEARCH: &str = include_str!("fixtures/chum-brave-search.toml");
const SQLITE_BINARY: &str = include_str!("fixtures/chum-sqlite.toml");
const LOCAL_DEV: &str = include_str!("fixtures/chum-local-dev.toml");
const POSTGRES_HTTP: &str = include_str!("fixtures/chum-postgres-remote.toml");

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
fn permissions_placeholder_round_trips_arbitrary_content() {
    // The filesystem fixture carries a `[permissions]` block with v0.2-shaped
    // content (allowed_paths / denied_paths). v0.1 chum-core does not
    // interpret it — but it must accept, expose, and round-trip the content
    // verbatim. Future v0.2 chum-core that introduces typed permissions will
    // bump schema_version, so this freeform-table behaviour does not stay
    // freeform forever.
    let manifest = parse_str(FILESYSTEM).expect("parse");
    let perms = manifest
        .permissions
        .as_ref()
        .expect("filesystem fixture should carry a [permissions] block");
    assert!(perms.contains_key("allowed_paths"));
    assert!(perms.contains_key("denied_paths"));

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
