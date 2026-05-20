//! Top-level install dispatch: [`chum_core::Manifest`] → [`InstalledArtifact`].
//!
//! v0.1 currently exposes only the data types here; the `install`
//! function and `InstallConfig` land in a later commit.

use std::path::PathBuf;

/// Result of a successful install.
///
/// Pure data — the daemon and registry consume this. `chum-install`
/// does not persist it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledArtifact {
    /// Package name, mirrored from `manifest.package.name`.
    pub name: String,
    /// Package version, mirrored from `manifest.package.version`.
    pub version: String,
    /// Absolute path to the per-package install directory. Always
    /// `<root>/packages/<name>/<version>/`.
    pub install_dir: PathBuf,
    /// Source-kind-specific entrypoint path. The daemon's start command
    /// resolves the server binary / module from here:
    ///
    /// - [`SourceKind::Npm`] → `install_dir/node_modules/<package>`
    /// - [`SourceKind::Local`] → `install_dir/local-src` (the symlink)
    /// - [`SourceKind::Binary`] → `install_dir/bin` (extracted archive
    ///   root or single-file binary)
    pub entrypoint: PathBuf,
    /// Source-kind tag without the original payload. Sufficient for
    /// the daemon to know which start pattern to use.
    pub source_kind: SourceKind,
}

/// Source-kind tag mirroring [`chum_core::manifest::Source`] variants
/// without their payload.
///
/// `#[non_exhaustive]` from day one: future variants (Pypi, Github,
/// Registry) extend this enum without a major version bump. Downstream
/// matches must include a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceKind {
    /// `npm`-installed package.
    Npm,
    /// Symlinked local working tree.
    Local,
    /// Downloaded and (optionally) extracted binary.
    Binary,
}
