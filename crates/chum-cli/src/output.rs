//! User-facing success output. Pairs with [`crate::error::render`] for
//! failures.
//!
//! Two modes:
//!
//! - **Human** — single-line `Installed <name> <version> at <dir>` on
//!   stdout. The default.
//! - **JSON** — a stable envelope on stdout consumed by scripts.
//!   Shape is locked at v0.1 and documented inline below.

use std::path::Path;

use chum_core::Manifest;
use chum_install::{InstalledArtifact, SourceKind};

/// Stable string form for [`SourceKind`] used in JSON output and
/// shared with the registry's column encoding.
fn source_kind_str(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Npm => "npm",
        SourceKind::Local => "local",
        SourceKind::Binary => "binary",
        _ => "unknown",
    }
}

/// Print confirmation that a manifest was installed and persisted.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "installed": {
///     "name": "...",
///     "version": "...",
///     "install_dir": "...",
///     "entrypoint": "...",
///     "source_kind": "npm|local|binary"
///   }
/// }
/// ```
///
/// The registry-assigned `id` is deliberately omitted — it is an
/// internal detail. Everything a script needs to chain a subsequent
/// `chum uninstall <name>` or `chum list` is here.
pub fn emit_installed(artifact: &InstalledArtifact, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "installed": {
                "name": artifact.name,
                "version": artifact.version,
                "install_dir": artifact.install_dir.display().to_string(),
                "entrypoint": artifact.entrypoint.display().to_string(),
                "source_kind": source_kind_str(artifact.source_kind),
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "Installed {} {} at {}",
            artifact.name,
            artifact.version,
            artifact.install_dir.display()
        );
    }
}

/// Print what `chum install` would do without touching the disk.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "dry-run",
///   "manifest": { "name": "...", "version": "..." },
///   "root": "...",
///   "would_install_at": "<root>/packages/<name>/<version>"
/// }
/// ```
///
/// Echoing the resolved `root` lets the caller verify that a
/// `--root` override was honored.
pub fn emit_dry_run(manifest: &Manifest, root: &Path, json: bool) {
    let would_install_at = root
        .join("packages")
        .join(&manifest.package.name)
        .join(&manifest.package.version);

    if json {
        let envelope = serde_json::json!({
            "status": "dry-run",
            "manifest": {
                "name": manifest.package.name,
                "version": manifest.package.version,
            },
            "root": root.display().to_string(),
            "would_install_at": would_install_at.display().to_string(),
        });
        println!("{envelope}");
    } else {
        println!(
            "Dry run: would install {} {} under {} (target: {})",
            manifest.package.name,
            manifest.package.version,
            root.display(),
            would_install_at.display(),
        );
    }
}
