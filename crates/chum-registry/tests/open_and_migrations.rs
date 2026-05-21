//! Integration tests covering [`Registry::open`] and the schema
//! migration runner.

mod common;

use chum_registry::{CURRENT_SCHEMA_VERSION, Registry};

use crate::common::TestDb;

#[test]
fn open_creates_db_if_missing() {
    let db = TestDb::new();
    assert!(
        !db.path.exists(),
        "fresh tempdir path should not contain state.db yet"
    );

    let registry = Registry::open(&db.path).expect("open should create the database");
    drop(registry);

    assert!(
        db.path.exists(),
        "Registry::open must create state.db when it does not exist"
    );
}

#[test]
fn open_runs_migrations_to_current_version() {
    let db = TestDb::new();
    let registry = Registry::open(&db.path).expect("open");

    let version = registry
        .schema_version()
        .expect("schema_version after open");

    assert_eq!(
        version, CURRENT_SCHEMA_VERSION,
        "open must advance the database to CURRENT_SCHEMA_VERSION"
    );
}

#[test]
fn schema_version_pinned_after_open() {
    let db = TestDb::new();

    // First open: migrations apply and advance schema_version from 0.
    {
        let r = Registry::open(&db.path).expect("first open");
        assert_eq!(r.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    // Re-opening the same file is idempotent — version stays pinned,
    // no migration runs twice.
    let r = Registry::open(&db.path).expect("second open");
    assert_eq!(r.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
}
