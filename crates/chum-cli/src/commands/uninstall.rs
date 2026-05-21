//! `chum uninstall` — remove an installed package's files and registry row.
//!
//! v0.1 stopgap: the cli touches the filesystem and the registry directly.
//! Once `chum-daemon` ships, uninstall sends a daemon protocol request
//! and the daemon owns the teardown.
//!
// TODO(chum-v0.x): route through chum-daemon protocol once it lands.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use clap::Args;
use chum_registry::{RegistryArtifact, RegistryError};

use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum uninstall`.
#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Package name to uninstall.
    pub name: String,

    /// Optional positional version. When omitted, the registry is
    /// consulted: 0 matches → `not_installed`, 1 → that one is used,
    /// 2+ → `ambiguous_version`.
    pub version: Option<String>,

    /// Same as the positional `version` but as a flag for scripts.
    /// If both are given the flag wins.
    #[arg(long = "version")]
    pub version_flag: Option<String>,

    /// Leave `install_dir` on disk; only remove the registry row.
    /// Useful for diagnostics or when the filesystem teardown is
    /// owned by something other than chum.
    #[arg(long)]
    pub keep_files: bool,

    /// Skip the interactive y/N confirmation. Required when stdin is
    /// not a tty for scripts that want belt-and-braces — without
    /// `--force` the cli will still proceed in non-tty mode, but
    /// passing `--force` makes the intent explicit.
    #[arg(long)]
    pub force: bool,

    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Emit machine-readable JSON. Implies `--force` in that the
    /// y/N prompt is skipped — script callers cannot answer it.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum uninstall`.
pub async fn run(args: UninstallArgs) -> Result<(), UserFacingError> {
    let root = crate::commands::resolve_root(args.root.clone())?;
    let db = root.join("state.db");

    if !db.is_file() {
        return Err(UserFacingError::NotInstalled {
            name: args.name.clone(),
        });
    }
    let registry = chum_registry::Registry::open(&db).map_err(UserFacingError::Registry)?;

    let target = pick_target(&registry, &args)?;

    if !should_proceed(&target, args.force, args.json) {
        output::emit_uninstall_cancelled(&target, args.json);
        return Ok(());
    }

    if !args.keep_files {
        match std::fs::remove_dir_all(&target.install_dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Idempotent: a previous `--keep-files` uninstall may
                // have already removed the row while the dir was
                // hand-deleted later. Don't fail the registry cleanup.
            }
            Err(source) => {
                return Err(UserFacingError::RemoveFailed {
                    path: target.install_dir.clone(),
                    source,
                });
            }
        }
    }

    registry
        .delete(&target.name, &target.version)
        .map_err(UserFacingError::Registry)?;

    output::emit_uninstalled(&target, args.keep_files, args.json);
    Ok(())
}

/// Pick which row to remove given the args.
///
/// - Explicit version: `get_by_name_version`; NotFound → `not_installed`.
/// - No version: `list_by_name`; 0 rows → `not_installed`; 1 row → use it;
///   2+ rows → `ambiguous_version` carrying the version list.
fn pick_target(
    registry: &chum_registry::Registry,
    args: &UninstallArgs,
) -> Result<RegistryArtifact, UserFacingError> {
    let resolved_version = args.version_flag.clone().or_else(|| args.version.clone());

    match resolved_version {
        Some(v) => match registry.get_by_name_version(&args.name, &v) {
            Ok(row) => Ok(row),
            Err(RegistryError::NotFound { .. }) => Err(UserFacingError::NotInstalled {
                name: args.name.clone(),
            }),
            Err(e) => Err(UserFacingError::Registry(e)),
        },
        None => {
            let mut matches = registry
                .list_by_name(&args.name)
                .map_err(UserFacingError::Registry)?;
            if matches.is_empty() {
                return Err(UserFacingError::NotInstalled {
                    name: args.name.clone(),
                });
            }
            if matches.len() > 1 {
                let versions = matches.into_iter().map(|r| r.version).collect();
                return Err(UserFacingError::AmbiguousVersion {
                    name: args.name.clone(),
                    versions,
                });
            }
            // `len() == 1` checked above; swap_remove(0) is total.
            Ok(matches.swap_remove(0))
        }
    }
}

/// Decide whether to actually perform the uninstall, asking the user
/// when appropriate.
///
/// Skips the prompt — returns `true` — if any of:
/// - `--force` was passed,
/// - `--json` was passed (scripts cannot answer interactive prompts),
/// - stdin is not a tty (also a script-like context).
///
/// In the tty + interactive path, anything other than `y`/`yes`
/// (case-insensitive) is treated as "no". An I/O failure on `read_line`
/// is also treated as "no" — safer to abandon than to remove silently.
fn should_proceed(target: &RegistryArtifact, force: bool, json: bool) -> bool {
    if force || json || !io::stdin().is_terminal() {
        return true;
    }
    print!("Remove {} {}? [y/N] ", target.name, target.version);
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    )
}
