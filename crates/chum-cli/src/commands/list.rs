//! `chum list` — read installed-artifact rows from the registry.
//!
//! v0.1 stopgap: the cli opens the SQLite registry directly. Once
//! `chum-daemon` ships, list moves behind a daemon protocol call.
//!
// TODO(chum-v0.x): route through chum-daemon protocol once it lands.

use std::path::PathBuf;

use clap::Args;

use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum list`.
#[derive(Args, Debug)]
pub struct ListArgs {
    /// Optional package name prefix to filter by. Pure `starts_with`
    /// — no glob, no regex.
    pub name_prefix: Option<String>,

    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Emit machine-readable JSON on stdout instead of a human table.
    #[arg(long)]
    pub json: bool,

    /// Disable ANSI color escapes even when stdout is a tty.
    #[arg(long)]
    pub no_color: bool,
}

/// Execute `chum list`.
///
/// Steps:
/// 1. Resolve the CHUM root (`--root` else `chum_home()`).
/// 2. If `<root>/state.db` does not exist, treat the install set as
///    empty — not an error. (A bare `chum list` on a fresh machine
///    should print "No packages installed", not "io error".)
/// 3. Otherwise open the registry and read every row.
/// 4. Apply the optional `name_prefix` filter.
/// 5. Sort by `installed_at` ascending (explicit; defends against
///    delete-and-reinsert races where the registry `id` order
///    diverges from install order).
/// 6. Hand off to [`output::emit_list`].
pub async fn run(args: ListArgs) -> Result<(), UserFacingError> {
    let root = crate::commands::resolve_root(args.root.clone())?;
    let db = root.join("state.db");

    let mut rows = if db.is_file() {
        let registry = chum_registry::Registry::open(&db).map_err(UserFacingError::Registry)?;
        registry.list_all().map_err(UserFacingError::Registry)?
    } else {
        Vec::new()
    };

    if let Some(prefix) = &args.name_prefix {
        rows.retain(|r| r.name.starts_with(prefix));
    }
    rows.sort_by(|a, b| a.installed_at.cmp(&b.installed_at));

    output::emit_list(&rows, &root, args.json, args.no_color);
    Ok(())
}
