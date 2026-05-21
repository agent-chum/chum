//! Top-level install dispatch: [`chum_core::Manifest`] → [`InstalledArtifact`].

use std::path::{Path, PathBuf};

use chum_core::Manifest;
use chum_core::manifest::Source;

use crate::Fetcher;
use crate::InstallError;

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

/// Tunables for [`install`]. Built via [`InstallConfig::default`] plus
/// `with_*` builder methods — `#[non_exhaustive]` means downstream
/// crates cannot construct via struct literal, so future fields
/// (`uvx_command` for v0.1.x pypi support, `git_command` for v0.2
/// github support, etc.) extend this struct without breaking callers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InstallConfig {
    /// Command + leading args to use as the `npm` binary.
    ///
    /// Production default: `vec!["npm".to_string()]` — invokes `npm`
    /// from `$PATH`.
    ///
    /// Integration tests override with e.g.
    /// `vec!["/bin/sh".into(), "<fake-npm-script>".into()]` so the
    /// same code path runs without a real npm install on the host.
    pub npm_command: Vec<String>,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            npm_command: vec!["npm".to_string()],
        }
    }
}

impl InstallConfig {
    /// Override the `npm_command`. Returns `self` for chaining.
    pub fn with_npm_command(mut self, npm_command: Vec<String>) -> Self {
        self.npm_command = npm_command;
        self
    }
}

/// Install a manifest under `root`.
///
/// Dispatches on `manifest.source`:
///
/// - [`Source::Npm`] → [`crate::sources::npm::install_npm`]
/// - [`Source::Local`] → [`crate::sources::local::install_local`]
/// - [`Source::Binary`] → [`crate::sources::binary::install_binary`]
///
/// [`Source::Pypi`] and [`Source::Github`] are accepted by the
/// manifest parser but rejected here with
/// [`InstallError::UnsupportedSource`] — they land in later versions.
///
/// `root` is typically [`crate::chum_home`]; tests pass a tempdir.
/// The function creates `<root>/packages/<name>/<version>/` if it
/// does not exist, then hands off to the kind-specific handler.
///
/// # Errors
///
/// Any [`InstallError`] variant the underlying handler can produce.
pub async fn install<F: Fetcher>(
    manifest: &Manifest,
    root: &Path,
    fetcher: &F,
    config: &InstallConfig,
) -> Result<InstalledArtifact, InstallError> {
    let install_dir = crate::paths::package_dir(
        root,
        &manifest.package.name,
        &manifest.package.version,
    );
    tokio::fs::create_dir_all(&install_dir).await?;

    let artifact = match &manifest.source {
        Source::Npm { package, version } => {
            crate::sources::npm::install_npm(
                manifest,
                &install_dir,
                package,
                version,
                &config.npm_command,
            )
            .await
        }
        Source::Local { path } => {
            crate::sources::local::install_local(manifest, &install_dir, path).await
        }
        Source::Binary {
            url,
            checksum_sha256,
            ..
        } => {
            crate::sources::binary::install_binary(
                manifest,
                &install_dir,
                url,
                checksum_sha256,
                fetcher,
            )
            .await
        }
        Source::Pypi { .. } => Err(InstallError::UnsupportedSource("pypi".to_string())),
        Source::Github { .. } => Err(InstallError::UnsupportedSource("github".to_string())),
    }?;

    // Post-install: write the manifest into the install dir + create
    // logs/. The daemon re-parses chum-manifest.toml on every spawn,
    // and per-package log files land in logs/{stdout,stderr}.log.
    write_post_install_artifacts(manifest, &artifact.install_dir).await?;

    Ok(artifact)
}

/// Serialize the manifest into `<install_dir>/chum-manifest.toml` and
/// create the empty `<install_dir>/logs/` directory.
///
/// This is the canonical post-install side-effect that
/// `chum-daemon` depends on: every spawn re-reads
/// `chum-manifest.toml` so the daemon sees exactly what the user
/// installed, and child stdout/stderr land in
/// `logs/{stdout,stderr}.log`.
///
/// Simple write (not atomic temp-file + rename) because
/// `install_dir` is private to a single install — no concurrent
/// writer to race against.
async fn write_post_install_artifacts(
    manifest: &Manifest,
    install_dir: &Path,
) -> Result<(), InstallError> {
    let manifest_toml = toml::to_string(manifest)
        .map_err(InstallError::ManifestSerialize)?;
    tokio::fs::write(install_dir.join("chum-manifest.toml"), manifest_toml).await?;
    tokio::fs::create_dir_all(install_dir.join("logs")).await?;
    Ok(())
}
