//! The [`Registry`] type — read/write SQLite-backed installed-artifact store.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use chum_install::{InstalledArtifact, SourceKind};
use rusqlite::{Connection, params};

use crate::error::RegistryError;
use crate::schema;
use crate::types::RegistryArtifact;

/// Read/write handle to the CHUM registry SQLite database.
///
/// Wraps a single [`rusqlite::Connection`] — not [`Sync`]. The daemon
/// owns one `Registry` instance and serialises access; tests create
/// one per `TempDir`.
pub struct Registry {
    conn: Connection,
}

impl Registry {
    /// Open or create the registry at `path`.
    ///
    /// Creates the SQLite file if it doesn't exist, enables foreign-key
    /// enforcement (no FKs in v0.1, but locked in so future migrations
    /// can rely on it), then runs schema migrations up to
    /// [`schema::CURRENT_SCHEMA_VERSION`].
    ///
    /// # Errors
    /// - [`RegistryError::SqlError`] if SQLite can't open the file or
    ///   apply pragmas.
    /// - [`RegistryError::MigrationFailed`] if the migration runner
    ///   fails or the on-disk database is at a higher schema version
    ///   than this binary knows about.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let mut conn = Connection::open(path.as_ref())?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Return the current `schema_version` recorded in the database.
    ///
    /// After a successful [`Registry::open`], this is always equal to
    /// [`schema::CURRENT_SCHEMA_VERSION`]. Exposed for diagnostics and
    /// for tests that need to verify the migration runner advanced
    /// past zero.
    pub fn schema_version(&self) -> Result<i64, RegistryError> {
        schema::read_schema_version(&self.conn)
    }

    /// Insert a fresh artifact record.
    ///
    /// Stamps `installed_at = Utc::now()`. This timestamp is registry-
    /// owned rather than caller-provided: `chum-install`'s
    /// [`InstalledArtifact`] describes *what landed on disk*, while
    /// *when it was recorded* is a registry fact. Forcing every caller
    /// to pull `chrono` would leak a timestamp concern across the
    /// install/registry boundary documented in `ARCHITECTURE.md`.
    ///
    /// # Errors
    /// - [`RegistryError::DuplicateArtifact`] on UNIQUE(name, version)
    ///   collision.
    /// - [`RegistryError::Io`] if `install_dir` or `entrypoint` is not
    ///   valid UTF-8 (TEXT columns require UTF-8).
    /// - [`RegistryError::SqlError`] for any other SQLite failure.
    pub fn insert(&self, artifact: &InstalledArtifact) -> Result<i64, RegistryError> {
        let install_dir = path_to_str(&artifact.install_dir)?;
        let entrypoint = path_to_str(&artifact.entrypoint)?;
        let source_kind = source_kind_to_str(artifact.source_kind)?;
        let installed_at: DateTime<Utc> = Utc::now();

        let result = self.conn.execute(
            "INSERT INTO installed_artifacts \
             (name, version, install_dir, entrypoint, source_kind, installed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                artifact.name,
                artifact.version,
                install_dir,
                entrypoint,
                source_kind,
                installed_at,
            ],
        );

        match result {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(RegistryError::DuplicateArtifact {
                    name: artifact.name.clone(),
                    version: artifact.version.clone(),
                })
            }
            Err(e) => Err(RegistryError::SqlError(e)),
        }
    }

    /// Fetch a single artifact by `(name, version)`.
    ///
    /// # Errors
    /// - [`RegistryError::NotFound`] if no row matches.
    /// - [`RegistryError::SqlError`] for any other SQLite failure.
    pub fn get_by_name_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<RegistryArtifact, RegistryError> {
        let row = self.conn.query_row(
            "SELECT id, name, version, install_dir, entrypoint, source_kind, installed_at \
             FROM installed_artifacts WHERE name = ?1 AND version = ?2",
            params![name, version],
            row_to_artifact,
        );
        match row {
            Ok(art) => Ok(art),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(RegistryError::NotFound {
                name: name.to_string(),
                version: version.to_string(),
            }),
            Err(e) => Err(RegistryError::SqlError(e)),
        }
    }

    /// List every row, ordered by `id` ascending (insertion order).
    ///
    /// Returns an empty `Vec` when the table is empty — this is not an
    /// error.
    pub fn list_all(&self) -> Result<Vec<RegistryArtifact>, RegistryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, install_dir, entrypoint, source_kind, installed_at \
             FROM installed_artifacts ORDER BY id ASC",
        )?;
        let rows = stmt.query_map((), row_to_artifact)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// List every row whose `name` matches, ordered by `id` ascending.
    ///
    /// Returns an empty `Vec` when no row has that name — this is not
    /// an error.
    pub fn list_by_name(&self, name: &str) -> Result<Vec<RegistryArtifact>, RegistryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, install_dir, entrypoint, source_kind, installed_at \
             FROM installed_artifacts WHERE name = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![name], row_to_artifact)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete the row matching `(name, version)`.
    ///
    /// # Errors
    /// - [`RegistryError::NotFound`] if no row matches.
    /// - [`RegistryError::SqlError`] for any other SQLite failure.
    pub fn delete(&self, name: &str, version: &str) -> Result<(), RegistryError> {
        // TODO(chum-v0.2): consider returning `bool` (deleted vs no-op)
        // so the daemon can distinguish "definitely gone" from "was
        // never there." For v0.1, NotFound is the right shape because
        // the CLI flow does an existence check before calling delete.
        let affected = self.conn.execute(
            "DELETE FROM installed_artifacts WHERE name = ?1 AND version = ?2",
            params![name, version],
        )?;
        if affected == 0 {
            Err(RegistryError::NotFound {
                name: name.to_string(),
                version: version.to_string(),
            })
        } else {
            Ok(())
        }
    }
}

fn path_to_str(p: &Path) -> Result<&str, RegistryError> {
    p.to_str().ok_or_else(|| {
        RegistryError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("path is not valid UTF-8: {}", p.display()),
        ))
    })
}

fn source_kind_to_str(kind: SourceKind) -> Result<&'static str, RegistryError> {
    // `SourceKind` is `#[non_exhaustive]` upstream — a future variant
    // (Pypi, Github, Registry) will compile here without forcing this
    // arm to update. The wildcard arm exists to turn that case into a
    // typed migration error at runtime: a new install-side variant
    // needs a matching chum-registry migration that teaches the
    // CHECK constraint to accept its string form.
    match kind {
        SourceKind::Npm => Ok("npm"),
        SourceKind::Local => Ok("local"),
        SourceKind::Binary => Ok("binary"),
        _ => Err(RegistryError::MigrationFailed {
            reason: format!(
                "chum-install supplied SourceKind {kind:?}, which this registry build does not know how to store; add a migration that extends the source_kind CHECK constraint",
            ),
        }),
    }
}

fn row_to_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<RegistryArtifact> {
    let install_dir: String = row.get(3)?;
    let entrypoint: String = row.get(4)?;
    let source_kind_text: String = row.get(5)?;
    let installed_at: DateTime<Utc> = row.get(6)?;

    // The `source_kind` column has a CHECK constraint accepting only
    // 'npm' | 'local' | 'binary' (see migration_1_initial), and
    // run_migrations refuses to open a DB whose schema_version is
    // higher than this binary supports. Together they make the
    // fallthrough arm structurally unreachable on any DB this binary
    // ever opens cleanly. The error path stays for corrupt files /
    // future-binary downgrade scenarios. Do not promote this to a
    // dedicated `RegistryError::SourceKindUnknown` variant by reflex —
    // FromSqlConversionFailure is the idiomatic rusqlite escape
    // hatch and keeps the error enum aligned with the v0.1 spec.
    let source_kind = match source_kind_text.as_str() {
        "npm" => SourceKind::Npm,
        "local" => SourceKind::Local,
        "binary" => SourceKind::Binary,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                format!("unknown source_kind `{other}`").into(),
            ));
        }
    };

    Ok(RegistryArtifact {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        install_dir: PathBuf::from(install_dir),
        entrypoint: PathBuf::from(entrypoint),
        source_kind,
        installed_at,
    })
}
