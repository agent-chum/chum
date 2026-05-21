//! Schema definition and migration runner.
//!
//! Philosophy:
//! - Migrations are append-only. Once a migration ships, its SQL is
//!   frozen — bugs are fixed by adding a *new* migration on top.
//! - `schema_version` is a single-row integer table that the runner
//!   advances atomically inside each migration's transaction.
//! - The runner refuses to operate on a database whose version is
//!   *higher* than this binary knows about. That case means the user
//!   is running an older `chum-registry` against a `state.db` written
//!   by a newer one; silently downgrading would lose data.
//!
//! [`MIGRATIONS`] is the canonical list: slot `i` holds the migration
//! that brings the database from schema version `i` to `i + 1`. To
//! introduce a new migration, append to the slice and bump
//! [`CURRENT_SCHEMA_VERSION`] in the same commit.

use rusqlite::{Connection, Transaction};

use crate::error::RegistryError;

/// The schema version this build of `chum-registry` writes and expects.
///
/// Bump in lockstep with adding a new entry to the migrations table.
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

type Migration = fn(&Transaction<'_>) -> rusqlite::Result<()>;

/// All migrations, in order. Slot `i` migrates the database from
/// schema version `i` to `i + 1`. Append-only; existing entries are
/// immutable once shipped.
const MIGRATIONS: &[Migration] = &[migration_1_initial];

/// Run all pending migrations against `conn`.
///
/// Creates the `schema_version` table if missing, reads the current
/// version (an empty row is treated as 0), and applies each pending
/// migration inside its own transaction. The version is bumped inside
/// the same transaction, so a partial migration is rolled back as a
/// unit.
///
/// # Errors
/// - [`RegistryError::SqlError`] for any rusqlite-level failure.
/// - [`RegistryError::MigrationFailed`] if the database is at a higher
///   schema version than [`CURRENT_SCHEMA_VERSION`] or a migration
///   function returns an error.
pub(crate) fn run_migrations(conn: &mut Connection) -> Result<(), RegistryError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL
        )",
        (),
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO schema_version (id, version) VALUES (1, 0)",
        (),
    )?;

    let current: i64 = conn.query_row(
        "SELECT version FROM schema_version WHERE id = 1",
        (),
        |row| row.get(0),
    )?;

    if current > CURRENT_SCHEMA_VERSION {
        return Err(RegistryError::MigrationFailed {
            reason: format!(
                "database is at schema version {current}, but this binary supports up to {CURRENT_SCHEMA_VERSION}"
            ),
        });
    }

    for v in (current as usize)..MIGRATIONS.len() {
        let target = (v as i64) + 1;
        let tx = conn.transaction()?;
        MIGRATIONS[v](&tx).map_err(|e| RegistryError::MigrationFailed {
            reason: format!("migration to v{target} failed: {e}"),
        })?;
        tx.execute(
            "UPDATE schema_version SET version = ?1 WHERE id = 1",
            [target],
        )?;
        tx.commit()?;
    }

    Ok(())
}

/// Read the current `schema_version` row.
///
/// Callable only after [`run_migrations`] has bootstrapped the table.
pub(crate) fn read_schema_version(conn: &Connection) -> Result<i64, RegistryError> {
    let v = conn.query_row(
        "SELECT version FROM schema_version WHERE id = 1",
        (),
        |row| row.get(0),
    )?;
    Ok(v)
}

/// Migration 1: create the `installed_artifacts` table.
///
/// The `source_kind` CHECK constraint pins the v0.1 enum members
/// (`'npm' | 'local' | 'binary'`) so that an unknown value can never
/// arrive in a row written by this build. See `row_to_artifact` in
/// `registry.rs` for the matching read-side discussion.
fn migration_1_initial(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute(
        "CREATE TABLE installed_artifacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            install_dir TEXT NOT NULL,
            entrypoint TEXT NOT NULL,
            source_kind TEXT NOT NULL CHECK (source_kind IN ('npm', 'local', 'binary')),
            installed_at TEXT NOT NULL,
            UNIQUE(name, version)
        )",
        (),
    )?;
    Ok(())
}
