//! Row-shape types exposed by the registry.

use std::path::PathBuf;

use chum_install::{InstalledArtifact, SourceKind};

/// A single row from the `installed_artifacts` table.
///
/// Mirrors [`chum_install::InstalledArtifact`] plus two registry-owned
/// fields: the autoincremented `id` and the `installed_at` timestamp.
///
/// `installed_at` is stamped by the registry at insert time — see
/// [`crate::Registry::insert`]. Callers do not provide it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryArtifact {
    /// Primary key. Assigned by SQLite via AUTOINCREMENT.
    pub id: i64,
    /// Package name. Mirrors `InstalledArtifact::name`.
    pub name: String,
    /// Package version. Mirrors `InstalledArtifact::version`.
    pub version: String,
    /// Absolute install directory. Mirrors `InstalledArtifact::install_dir`.
    pub install_dir: PathBuf,
    /// Source-kind-specific entrypoint. Mirrors `InstalledArtifact::entrypoint`.
    pub entrypoint: PathBuf,
    /// Source-kind tag. Mirrors `InstalledArtifact::source_kind`.
    pub source_kind: SourceKind,
    /// UTC timestamp of the insert that wrote this row.
    pub installed_at: chrono::DateTime<chrono::Utc>,
}

impl From<RegistryArtifact> for InstalledArtifact {
    /// Project a [`RegistryArtifact`] down to the install-side shape.
    ///
    /// Lossy by design: `id` and `installed_at` are registry-owned and
    /// have no counterpart in [`InstalledArtifact`].
    fn from(row: RegistryArtifact) -> Self {
        Self {
            name: row.name,
            version: row.version,
            install_dir: row.install_dir,
            entrypoint: row.entrypoint,
            source_kind: row.source_kind,
        }
    }
}
