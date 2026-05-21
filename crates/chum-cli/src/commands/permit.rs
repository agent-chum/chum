//! `chum permit <name> --grant <kind>=<value>` — grant one or more
//! permissions to an installed package. Multiple `--grant` flags
//! accumulate.

use std::path::PathBuf;

use chum_core::PermissionKind;
use clap::Args;

use crate::commands::resolve_lifecycle_target;
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum permit`.
#[derive(Args, Debug)]
pub struct PermitArgs {
    /// Package name.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Grants in `<kind>=<value>` form (kind is one of:
    /// filesystem.read, filesystem.write, network.outbound,
    /// env.read, subprocess.exec). Pass once per grant.
    #[arg(long = "grant", value_name = "KIND=VALUE", required = true)]
    pub grants: Vec<String>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path. (Currently unused — `chum permit`
    /// writes the registry directly without going through chumd —
    /// kept for forward-compat with the v0.2 daemon-mediated grant
    /// path.)
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// Execute `chum permit`.
pub async fn run(args: PermitArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    // Parse every --grant string up-front so a typo on the third
    // grant doesn't half-apply the first two.
    let parsed: Vec<(PermissionKind, String)> = args
        .grants
        .iter()
        .map(|s| parse_grant(s))
        .collect::<Result<Vec<_>, _>>()?;

    let registry = chum_registry::Registry::open(target.socket_path_root_state_db()?)
        .map_err(UserFacingError::Registry)?;
    let row = registry
        .get_by_name_version(&target.name, &target.version)
        .map_err(UserFacingError::Registry)?;

    let mut applied = Vec::with_capacity(parsed.len());
    for (kind, value) in &parsed {
        registry
            .grant(row.id, kind.as_str(), value)
            .map_err(UserFacingError::Registry)?;
        applied.push((kind.as_str().to_string(), value.clone()));
    }

    output::emit_granted(&target.name, &target.version, &applied, args.json);
    Ok(())
}

/// Parse `<kind>=<value>` into a typed `(PermissionKind, String)`.
fn parse_grant(input: &str) -> Result<(PermissionKind, String), UserFacingError> {
    let (kind_str, value) =
        input
            .split_once('=')
            .ok_or_else(|| UserFacingError::UnknownPermission {
                input: input.to_string(),
            })?;
    let kind = PermissionKind::from_str(kind_str).ok_or_else(|| {
        UserFacingError::UnknownPermission {
            input: input.to_string(),
        }
    })?;
    if value.is_empty() {
        return Err(UserFacingError::UnknownPermission {
            input: input.to_string(),
        });
    }
    Ok((kind, value.to_string()))
}

impl crate::commands::LifecycleTarget {
    /// `<root>/state.db` derived from the resolved socket path's
    /// parent. The cli always passes socket_path = root/daemon.sock,
    /// so this is reliable; tests can override.
    pub(crate) fn socket_path_root_state_db(&self) -> Result<PathBuf, UserFacingError> {
        let parent = self
            .socket_path
            .parent()
            .ok_or_else(|| UserFacingError::ChumHomeUnresolved)?;
        Ok(parent.join("state.db"))
    }
}
