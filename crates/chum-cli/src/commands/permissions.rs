//! `chum permissions <name>` — show the three-section diff of
//! declared vs granted vs missing permissions for an installed
//! package.

use std::path::PathBuf;

use clap::Args;

use crate::commands::resolve_lifecycle_target;
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum permissions`.
#[derive(Args, Debug)]
pub struct PermissionsArgs {
    /// Package name.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path (unused today; see permit.rs).
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum permissions`.
pub async fn run(args: PermissionsArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    // Read the manifest from install_dir to recover declared
    // permissions — same source the daemon uses on spawn.
    let manifest_path = target.install_dir.join("chum-manifest.toml");
    let manifest_text =
        std::fs::read_to_string(&manifest_path).map_err(|_| UserFacingError::ManifestMissing {
            install_dir: target.install_dir.clone(),
        })?;
    let manifest = chum_core::parse_and_validate(&manifest_text)
        .map_err(UserFacingError::Manifest)?;

    let registry = chum_registry::Registry::open(target.socket_path_root_state_db()?)
        .map_err(UserFacingError::Registry)?;
    let row = registry
        .get_by_name_version(&target.name, &target.version)
        .map_err(UserFacingError::Registry)?;
    let grants = registry
        .list_grants(row.id)
        .map_err(UserFacingError::Registry)?;

    let declared: Vec<(String, String)> = manifest
        .permissions
        .iter_requirements()
        .map(|r| (r.kind.as_str().to_string(), r.value))
        .collect();
    let granted: Vec<(String, String)> = grants
        .iter()
        .map(|g| (g.kind.clone(), g.value.clone()))
        .collect();
    let missing: Vec<(String, String)> = declared
        .iter()
        .filter(|(k, v)| !granted.iter().any(|(gk, gv)| gk == k && gv == v))
        .cloned()
        .collect();

    output::emit_permissions(
        &target.name,
        &target.version,
        &declared,
        &grants,
        &missing,
        args.json,
    );
    Ok(())
}
