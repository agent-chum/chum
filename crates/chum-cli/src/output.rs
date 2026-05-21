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
use chum_registry::RegistryArtifact;

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

/// Render `install_dir` relative to the CHUM root for compact column
/// output. Falls back to the absolute path if `install_dir` somehow
/// lives outside `root` (shouldn't happen for registry-written rows).
fn relative_to_root<'a>(install_dir: &'a Path, root: &Path) -> std::path::PathBuf {
    install_dir
        .strip_prefix(root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| install_dir.to_path_buf())
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

/// Render the result of `chum list`.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "packages": [
///     {
///       "name": "...",
///       "version": "...",
///       "source_kind": "npm|local|binary",
///       "install_dir": "...",
///       "installed_at": "2026-05-21T13:30:00+00:00"
///     }
///   ]
/// }
/// ```
///
/// Human form: a fixed-width table with columns
/// `NAME | VERSION | KIND | INSTALLED | PATH`, where `PATH` is
/// rendered relative to `root` for compactness. An empty list prints
/// `No packages installed.` and exits 0.
pub fn emit_list(rows: &[RegistryArtifact], root: &Path, json: bool) {
    if json {
        let packages: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "version": r.version,
                    "source_kind": source_kind_str(r.source_kind),
                    "install_dir": r.install_dir.display().to_string(),
                    "installed_at": r.installed_at.to_rfc3339(),
                })
            })
            .collect();
        let envelope = serde_json::json!({
            "status": "ok",
            "packages": packages,
        });
        println!("{envelope}");
        return;
    }

    if rows.is_empty() {
        println!("No packages installed.");
        return;
    }

    let installed_fmt: Vec<String> = rows
        .iter()
        .map(|r| r.installed_at.format("%Y-%m-%d %H:%M").to_string())
        .collect();
    let paths: Vec<String> = rows
        .iter()
        .map(|r| relative_to_root(&r.install_dir, root).display().to_string())
        .collect();

    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let ver_w = rows
        .iter()
        .map(|r| r.version.len())
        .max()
        .unwrap_or(0)
        .max("VERSION".len());
    let kind_w = rows
        .iter()
        .map(|r| source_kind_str(r.source_kind).len())
        .max()
        .unwrap_or(0)
        .max("KIND".len());
    let installed_w = installed_fmt
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("INSTALLED".len());

    println!(
        "{:nw$}  {:vw$}  {:kw$}  {:iw$}  PATH",
        "NAME",
        "VERSION",
        "KIND",
        "INSTALLED",
        nw = name_w,
        vw = ver_w,
        kw = kind_w,
        iw = installed_w,
    );
    for (i, r) in rows.iter().enumerate() {
        println!(
            "{:nw$}  {:vw$}  {:kw$}  {:iw$}  {}",
            r.name,
            r.version,
            source_kind_str(r.source_kind),
            installed_fmt[i],
            paths[i],
            nw = name_w,
            vw = ver_w,
            kw = kind_w,
            iw = installed_w,
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
