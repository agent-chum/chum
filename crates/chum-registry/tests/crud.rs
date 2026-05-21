//! Integration tests covering CRUD operations and error paths on
//! [`Registry`].

mod common;

use chum_install::SourceKind;
use chum_registry::{Registry, RegistryError};

use crate::common::{TestDb, make_artifact};

#[test]
fn insert_then_get_roundtrip() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let art = make_artifact("foo", "1.0.0", SourceKind::Npm);

    let id = r.insert(&art).expect("insert should succeed");
    assert!(id > 0, "SQLite should assign a positive id, got {id}");

    let got = r
        .get_by_name_version("foo", "1.0.0")
        .expect("get_by_name_version should find the row");

    assert_eq!(got.id, id);
    assert_eq!(got.name, "foo");
    assert_eq!(got.version, "1.0.0");
    assert_eq!(got.install_dir, art.install_dir);
    assert_eq!(got.entrypoint, art.entrypoint);
    assert_eq!(got.source_kind, SourceKind::Npm);

    // `installed_at` is registry-stamped on insert. The exact value
    // is the registry's choice, but it should be close to "now" — a
    // 5-second window is generous for any reasonable test runner.
    let delta = (chrono::Utc::now() - got.installed_at)
        .num_seconds()
        .abs();
    assert!(
        delta < 5,
        "installed_at should be ~now (delta = {delta}s)"
    );
}

#[test]
fn insert_duplicate_returns_duplicate_artifact_error() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    let art = make_artifact("foo", "1.0.0", SourceKind::Npm);

    r.insert(&art).expect("first insert succeeds");
    let err = r
        .insert(&art)
        .expect_err("second insert on same (name, version) must fail");

    match err {
        RegistryError::DuplicateArtifact { name, version } => {
            assert_eq!(name, "foo");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected DuplicateArtifact, got {other:?}"),
    }
}

#[test]
fn list_all_returns_in_insertion_order() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();

    r.insert(&make_artifact("a-pkg", "1.0.0", SourceKind::Npm))
        .unwrap();
    r.insert(&make_artifact("b-pkg", "1.0.0", SourceKind::Local))
        .unwrap();
    r.insert(&make_artifact("c-pkg", "1.0.0", SourceKind::Binary))
        .unwrap();

    let rows = r.list_all().expect("list_all");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "a-pkg");
    assert_eq!(rows[1].name, "b-pkg");
    assert_eq!(rows[2].name, "c-pkg");

    assert!(
        rows[0].id < rows[1].id && rows[1].id < rows[2].id,
        "ids should monotonically increase with insertion order: {:?}",
        rows.iter().map(|r| r.id).collect::<Vec<_>>()
    );
}

#[test]
fn list_by_name_filters_correctly() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();

    r.insert(&make_artifact("a-pkg", "1.0.0", SourceKind::Npm))
        .unwrap();
    r.insert(&make_artifact("a-pkg", "2.0.0", SourceKind::Npm))
        .unwrap();
    r.insert(&make_artifact("b-pkg", "1.0.0", SourceKind::Local))
        .unwrap();

    let rows = r.list_by_name("a-pkg").expect("list_by_name");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.name == "a-pkg"));
    assert_eq!(rows[0].version, "1.0.0");
    assert_eq!(rows[1].version, "2.0.0");

    let none = r
        .list_by_name("missing-pkg")
        .expect("missing name returns empty, not error");
    assert!(none.is_empty());
}

#[test]
fn delete_removes_row() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();
    r.insert(&make_artifact("foo", "1.0.0", SourceKind::Npm))
        .unwrap();

    r.delete("foo", "1.0.0").expect("delete should succeed");

    match r.get_by_name_version("foo", "1.0.0") {
        Err(RegistryError::NotFound { name, version }) => {
            assert_eq!(name, "foo");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotFound after delete, got {other:?}"),
    }
}

#[test]
fn delete_nonexistent_returns_not_found() {
    let db = TestDb::new();
    let r = Registry::open(&db.path).unwrap();

    let err = r
        .delete("ghost", "0.0.0")
        .expect_err("delete on missing row must fail");

    match err {
        RegistryError::NotFound { name, version } => {
            assert_eq!(name, "ghost");
            assert_eq!(version, "0.0.0");
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}
