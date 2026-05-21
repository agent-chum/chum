//! `chum install` — wire a manifest through parse → install → persist.
//!
//! v0.1 stopgap: this module composes `chum-core` / `chum-install` /
//! `chum-registry` directly. Once `chum-daemon` ships, the CLI will
//! send an `Install` request over the daemon protocol instead.
//!
// TODO(chum-v0.x): route through chum-daemon protocol once it lands.
// The pipeline below stays the same shape; only the boundary moves.

use std::path::PathBuf;

use clap::Args;

use crate::error::UserFacingError;
use crate::output;

/// Arguments for `chum install`.
#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Path to the manifest TOML.
    pub manifest_path: PathBuf,

    /// Override CHUM_HOME for this invocation. Defaults to
    /// `$CHUM_HOME`, then `$XDG_DATA_HOME/chum`, then `$HOME/.chum`.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Parse and validate the manifest only; do not touch the
    /// filesystem or registry. The resolved root is echoed in the
    /// output so callers can confirm a `--root` override took effect.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit machine-readable JSON on stdout instead of human text.
    /// In `--json` mode error envelopes also go to stdout (with
    /// `status="error"`), so a single stream is enough for scripts.
    #[arg(long)]
    pub json: bool,
}

/// Execute the install pipeline.
///
/// Steps:
/// 1. Read + parse + validate the manifest.
/// 2. Resolve the CHUM root (`--root` override, else `chum_home()`).
/// 3. `--dry-run` short-circuit: print what would happen and exit.
/// 4. Create the root if missing, open the registry.
/// 5. Pre-check the registry for `(name, version)` — refuse to
///    overwrite an existing install.
/// 6. Hand off to `chum_install::install` to do the actual work.
/// 7. Persist the resulting [`chum_install::InstalledArtifact`] into
///    the registry and print confirmation.
pub async fn run(args: InstallArgs) -> Result<(), UserFacingError> {
    let manifest_text =
        std::fs::read_to_string(&args.manifest_path).map_err(|source| {
            UserFacingError::ManifestIo {
                path: args.manifest_path.clone(),
                source,
            }
        })?;
    let manifest =
        chum_core::parse_and_validate(&manifest_text).map_err(UserFacingError::Manifest)?;

    let root = crate::commands::resolve_root(args.root.clone())?;

    if args.dry_run {
        output::emit_dry_run(&manifest, &root, args.json);
        return Ok(());
    }

    std::fs::create_dir_all(&root).map_err(|source| UserFacingError::RootIo {
        path: root.clone(),
        source,
    })?;

    let registry = chum_registry::Registry::open(root.join("state.db"))
        .map_err(UserFacingError::Registry)?;

    match registry.get_by_name_version(&manifest.package.name, &manifest.package.version) {
        Ok(_) => {
            return Err(UserFacingError::AlreadyInstalled {
                name: manifest.package.name.clone(),
                version: manifest.package.version.clone(),
            });
        }
        Err(chum_registry::RegistryError::NotFound { .. }) => {}
        Err(e) => return Err(UserFacingError::Registry(e)),
    }

    let fetcher = chum_install::ReqwestFetcher::new().map_err(UserFacingError::Install)?;
    let config = chum_install::InstallConfig::default();
    let artifact = chum_install::install(&manifest, &root, &fetcher, &config)
        .await
        .map_err(UserFacingError::Install)?;

    registry.insert(&artifact).map_err(UserFacingError::Registry)?;
    output::emit_installed(&artifact, args.json);
    output::emit_install_permission_hint(&manifest, args.json);
    Ok(())
}
