//! Integration tests for the `permission_grants` table + the
//! `grant` / `revoke` / `list_grants` API surface.

mod common;

use chum_install::SourceKind;
use chum_registry::{CURRENT_SCHEMA_VERSION, Registry, RegistryError};

use crate::common::{TestDb, make_artifact};

#[test]
fn schema_version_advanced_to_two() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).expect("open");
    assert_eq!(r.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(CURRENT_SCHEMA_VERSION, 2);
}

#[test]
fn grant_then_list_round_trip() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();

    r.grant(id, "filesystem.read", "/Users/x/Documents").unwrap();
    r.grant(id, "filesystem.write", "/tmp/chum-workspace").unwrap();
    r.grant(id, "env.read", "BRAVE_API_KEY").unwrap();

    let grants = r.list_grants(id).expect("list_grants");
    assert_eq!(grants.len(), 3);
    let kinds: Vec<_> = grants.iter().map(|g| g.kind.as_str()).collect();
    assert!(kinds.contains(&"filesystem.read"));
    assert!(kinds.contains(&"filesystem.write"));
    assert!(kinds.contains(&"env.read"));
}

#[test]
fn grant_is_idempotent_on_duplicate() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();

    let first = r.grant(id, "env.read", "FOO").expect("first grant");
    let second = r.grant(id, "env.read", "FOO").expect("repeat is no-op");
    assert_eq!(first, second, "repeat grant returns the same row id");

    let grants = r.list_grants(id).unwrap();
    assert_eq!(grants.len(), 1, "no duplicate row inserted");
}

#[test]
fn grant_rejects_unknown_kind() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();

    let err = r
        .grant(id, "not.a.kind", "value")
        .expect_err("CHECK should reject unknown kind");
    match err {
        RegistryError::SqlError(_) => {}
        other => panic!("expected SqlError from CHECK violation, got {other:?}"),
    }
}

#[test]
fn revoke_removes_grant() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();
    r.grant(id, "env.read", "FOO").unwrap();

    r.revoke(id, "env.read", "FOO").expect("revoke");
    assert!(r.list_grants(id).unwrap().is_empty());
}

#[test]
fn revoke_missing_returns_not_found() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();

    let err = r
        .revoke(id, "env.read", "MISSING")
        .expect_err("revoke nonexistent must fail");
    match err {
        RegistryError::NotFound { name, version } => {
            assert_eq!(name, "env.read");
            assert_eq!(version, "MISSING");
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn uninstall_cascades_to_grants() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();
    r.grant(id, "env.read", "A").unwrap();
    r.grant(id, "env.read", "B").unwrap();
    assert_eq!(r.list_grants(id).unwrap().len(), 2);

    r.delete("foo", "1.0.0").unwrap();
    // After cascade, grants belong to a nonexistent artifact.
    // list_grants on the (now-deleted) id should return empty.
    assert_eq!(r.list_grants(id).unwrap().len(), 0);
}

#[test]
fn list_grants_by_name_version_resolves_artifact() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let id = r
        .insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();
    r.grant(id, "filesystem.read", "/x").unwrap();

    let by_id = r.list_grants(id).unwrap();
    let by_name = r.list_grants_by_name_version("foo", "1.0.0").unwrap();
    assert_eq!(by_id, by_name);

    let err = r
        .list_grants_by_name_version("ghost", "9.9")
        .expect_err("ghost artifact must error");
    match err {
        RegistryError::NotFound { .. } => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}
