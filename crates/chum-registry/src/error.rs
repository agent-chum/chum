//! Errors raised by chum-registry operations.

use thiserror::Error;

/// All errors emitted by `chum-registry`.
///
/// Variants are pattern-match-friendly: callers can distinguish a
/// duplicate insert from a missing row without parsing error messages.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Low-level SQLite error from `rusqlite`. Includes I/O on the
    /// SQLite file, malformed databases, prepared-statement failures,
    /// and `FromSqlConversionFailure` for any text column whose value
    /// fails to decode into its Rust counterpart.
    #[error("sqlite error: {0}")]
    SqlError(#[from] rusqlite::Error),

    /// The migration runner could not bring the database to the
    /// current schema version. Typical causes: the on-disk database
    /// is newer than this binary, or a migration script failed.
    #[error("schema migration failed: {reason}")]
    MigrationFailed {
        /// Free-form reason. Stable across patch versions; do not
        /// pattern-match on the string.
        reason: String,
    },

    /// A row matching `(name, version)` was expected but not present.
    /// Emitted by `get_by_name_version` and by `delete` when the
    /// targeted row does not exist.
    #[error("no artifact registered with name `{name}` and version `{version}`")]
    NotFound {
        /// Artifact name.
        name: String,
        /// Artifact version.
        version: String,
    },

    /// A row matching `(name, version)` already exists. Insert refused.
    #[error("artifact `{name}` version `{version}` is already registered")]
    DuplicateArtifact {
        /// Artifact name that collided.
        name: String,
        /// Artifact version that collided.
        version: String,
    },

    /// I/O error from `std` — chiefly: a path field on
    /// [`chum_install::InstalledArtifact`] that is not valid UTF-8
    /// (TEXT columns require UTF-8), or filesystem trouble locating
    /// the SQLite file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
