//! Local source handler: symlink an absolute path into the install dir.

use std::path::{Component, Path};

use chum_core::Manifest;

use crate::install::{InstalledArtifact, SourceKind};
use crate::InstallError;

/// Install a [`chum_core::manifest::Source::Local`] manifest by
/// symlinking its `path` into the per-package install directory.
///
/// # Validation
///
/// - `local_path` must be **absolute**. Relative paths are rejected as
///   a path-traversal attempt.
/// - `local_path` must contain no `..` components, even if otherwise
///   absolute. macOS resolves `..` through symlinks at the kernel
///   level; we refuse rather than try to canonicalise.
/// - The target must exist on the filesystem (file or directory).
///
/// # Behaviour
///
/// Creates a symlink at `install_dir/local-src` pointing at the
/// validated path. If a symlink or file already exists at that
/// location, it is removed first so the install is idempotent.
///
/// The symlink is **not** followed during install — the daemon resolves
/// it when starting the server. This keeps the install fast for large
/// working trees.
///
/// # Errors
///
/// - [`InstallError::PathTraversal`] for relative paths or paths
///   containing `..` components.
/// - [`InstallError::Io`] for a missing target, permission errors, or
///   symlink creation failures.
pub async fn install_local(
    manifest: &Manifest,
    install_dir: &Path,
    local_path: &str,
) -> Result<InstalledArtifact, InstallError> {
    let path = Path::new(local_path);

    if !path.is_absolute() {
        return Err(InstallError::PathTraversal(path.to_path_buf()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(InstallError::PathTraversal(path.to_path_buf()));
    }

    // Existence check. File or directory; either is fine.
    let _ = tokio::fs::metadata(path).await?;

    let symlink_target = install_dir.join("local-src");

    // Idempotent: remove any prior symlink / file at the target.
    if tokio::fs::symlink_metadata(&symlink_target).await.is_ok() {
        tokio::fs::remove_file(&symlink_target).await?;
    }

    tokio::fs::symlink(path, &symlink_target).await?;

    Ok(InstalledArtifact {
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        install_dir: install_dir.to_path_buf(),
        entrypoint: symlink_target,
        source_kind: SourceKind::Local,
    })
}
