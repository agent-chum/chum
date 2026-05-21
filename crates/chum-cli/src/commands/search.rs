//! `chum search [query]` — substring-search across first-party
//! manifests and installed packages.
//!
//! Precedence on name collision: installed wins. Versions and
//! descriptions come from the installed copy when a row exists in
//! the registry; otherwise from the first-party manifest at
//! `--manifests-dir`.

use std::path::PathBuf;

use clap::Args;

use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum search`.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Optional substring to filter results by. Matched
    /// case-insensitively against both name and description.
    pub query: Option<String>,
    /// Show only installed packages.
    #[arg(long, conflicts_with = "available_only")]
    pub installed_only: bool,
    /// Show only available (not-installed) packages.
    #[arg(long)]
    pub available_only: bool,
    /// Path to the first-party manifests directory. Defaults to
    /// `./manifests/` relative to the current working directory.
    /// v0.2 will bake first-party manifests into the binary.
    #[arg(long)]
    pub manifests_dir: Option<PathBuf>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// One search result row.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Package name.
    pub name: String,
    /// Resolved version: installed registry version if present,
    /// else first-party manifest version.
    pub version: String,
    /// `"installed"` or `"available"`.
    pub status: &'static str,
    /// `[package].description` from whichever manifest provided the
    /// resolution.
    pub description: String,
}

/// Execute `chum search`.
pub async fn run(args: SearchArgs) -> Result<(), UserFacingError> {
    // First-party manifests dir — silently skipped if missing.
    let manifests_dir = args
        .manifests_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("manifests"));
    let first_party = read_first_party(&manifests_dir);

    // Installed packages from the registry. Same default-empty
    // behavior as `chum list` — missing state.db is not an error.
    let installed = read_installed(args.root.clone())?;

    // Union by name, installed wins.
    let mut by_name: std::collections::BTreeMap<String, SearchResult> = Default::default();
    for fp in first_party {
        by_name.insert(fp.name.clone(), fp);
    }
    for inst in installed {
        by_name.insert(inst.name.clone(), inst);
    }

    let mut results: Vec<SearchResult> = by_name.into_values().collect();

    if let Some(q) = &args.query {
        let q_lower = q.to_lowercase();
        results.retain(|r| {
            r.name.to_lowercase().contains(&q_lower)
                || r.description.to_lowercase().contains(&q_lower)
        });
    }
    if args.installed_only {
        results.retain(|r| r.status == "installed");
    }
    if args.available_only {
        results.retain(|r| r.status == "available");
    }
    results.sort_by(|a, b| a.name.cmp(&b.name));

    output::emit_search(&results, args.json);
    Ok(())
}

fn read_first_party(dir: &std::path::Path) -> Vec<SearchResult> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(), // missing dir is silent — not an error
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = chum_core::parse_and_validate(&body) else {
            continue;
        };
        out.push(SearchResult {
            name: manifest.package.name,
            version: manifest.package.version,
            status: "available",
            description: manifest.package.description,
        });
    }
    out
}

fn read_installed(root_arg: Option<PathBuf>) -> Result<Vec<SearchResult>, UserFacingError> {
    let root = crate::commands::resolve_root(root_arg)?;
    let db = root.join("state.db");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let registry = chum_registry::Registry::open(&db).map_err(UserFacingError::Registry)?;
    let rows = registry.list_all().map_err(UserFacingError::Registry)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        // Description comes from the installed copy of the manifest
        // (which may differ from the first-party manifest's
        // description if the user installed a customised one).
        let manifest_path = row.install_dir.join("chum-manifest.toml");
        let description = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| chum_core::parse_and_validate(&t).ok())
            .map(|m| m.package.description)
            .unwrap_or_default();
        out.push(SearchResult {
            name: row.name,
            version: row.version,
            status: "installed",
            description,
        });
    }
    Ok(out)
}
