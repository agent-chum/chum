//! `chum revoke <name> --grant <kind>=<value>` — remove one grant.

use std::path::PathBuf;

use chum_core::PermissionKind;
use clap::Args;

use crate::commands::resolve_lifecycle_target;
use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum revoke`.
#[derive(Args, Debug)]
pub struct RevokeArgs {
    /// Package name.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Grant to revoke in `<kind>=<value>` form. v0.1 supports one
    /// grant per invocation.
    #[arg(long = "grant", value_name = "KIND=VALUE", required = true)]
    pub grant: String,
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

/// Execute `chum revoke`.
pub async fn run(args: RevokeArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        args.socket_path.clone(),
    )?;

    let (kind, value) = parse_grant(&args.grant)?;

    let registry = chum_registry::Registry::open(target.socket_path_root_state_db()?)
        .map_err(UserFacingError::Registry)?;
    let row = registry
        .get_by_name_version(&target.name, &target.version)
        .map_err(UserFacingError::Registry)?;

    match registry.revoke(row.id, kind.as_str(), &value) {
        Ok(()) => {
            output::emit_revoked(&target.name, &target.version, kind.as_str(), &value, args.json);
            Ok(())
        }
        Err(chum_registry::RegistryError::NotFound { .. }) => {
            Err(UserFacingError::GrantNotFound {
                name: target.name.clone(),
                version: target.version.clone(),
                kind: kind.as_str().to_string(),
                value,
            })
        }
        Err(e) => Err(UserFacingError::Registry(e)),
    }
}

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
