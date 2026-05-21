//! `chum env list|set|unset <name>` — manage a package's
//! `runtime.env` block in its `<install_dir>/chum-manifest.toml`.
//!
//! v0.1 design notes:
//! - Values are written verbatim to the on-disk manifest. The daemon
//!   re-reads `chum-manifest.toml` on every spawn, so an env change
//!   takes effect on the **next** `chum start` (or `chum restart`).
//!   In-flight processes do not see env changes mid-flight.
//! - `list` shows keys only — never values. Secrets stay opaque even
//!   in `--json` output.
//! - Status column for `list` unions
//!   `permissions.env.read` (declared) with `runtime.env.keys()` (set):
//!   each key shows `set` or `unset`.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::commands::resolve_lifecycle_target;
use crate::error::UserFacingError;
use crate::output;

/// Arguments shared by every `chum env <sub>` invocation.
#[derive(Args, Debug, Clone)]
pub struct EnvCommonArgs {
    /// Package name.
    pub name: String,
    /// Explicit version; required when more than one is installed.
    #[arg(long)]
    pub version: Option<String>,
    /// Override CHUM_HOME for this invocation.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Emit machine-readable JSON on stdout.
    #[arg(long)]
    pub json: bool,
}

/// `chum env <sub>` subcommands.
#[derive(Subcommand, Debug)]
pub enum EnvSub {
    /// Show declared + set env keys for an installed package. Values
    /// are never printed.
    List(EnvCommonArgs),
    /// Set or overwrite an env entry in the manifest's `runtime.env`.
    Set {
        #[command(flatten)]
        common: EnvCommonArgs,
        /// `KEY=VALUE` form. Value can contain `=` (split on the
        /// first `=` only).
        kv: String,
    },
    /// Remove an env entry from `runtime.env`. Idempotent — removing
    /// a key that wasn't there is success.
    Unset {
        #[command(flatten)]
        common: EnvCommonArgs,
        /// Env var name to remove.
        key: String,
    },
}

/// Top-level dispatch for `chum env`.
pub async fn run(sub: EnvSub) -> Result<(), UserFacingError> {
    match sub {
        EnvSub::List(common) => list(common).await,
        EnvSub::Set { common, kv } => set(common, kv).await,
        EnvSub::Unset { common, key } => unset(common, key).await,
    }
}

async fn list(args: EnvCommonArgs) -> Result<(), UserFacingError> {
    let target = resolve_lifecycle_target(
        &args.name,
        args.version.as_deref(),
        args.root.clone(),
        None,
    )?;
    let manifest = read_manifest_at(&target.install_dir)?;

    // Union: keys declared in permissions.env.read + keys set in runtime.env.
    let mut keys: Vec<String> = manifest.permissions.env.read.clone();
    for k in manifest.runtime.env.keys() {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    keys.sort();

    let entries: Vec<(String, bool)> = keys
        .into_iter()
        .map(|k| {
            let set = manifest.runtime.env.contains_key(&k);
            (k, set)
        })
        .collect();

    output::emit_env_list(&target.name, &target.version, &entries, args.json);
    Ok(())
}

async fn set(common: EnvCommonArgs, kv: String) -> Result<(), UserFacingError> {
    let (key, value) =
        kv.split_once('=')
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .ok_or_else(|| UserFacingError::EnvKeyInvalid { key: kv.clone() })?;

    if !is_valid_env_key(&key) {
        return Err(UserFacingError::EnvKeyInvalid { key });
    }

    let target = resolve_lifecycle_target(
        &common.name,
        common.version.as_deref(),
        common.root.clone(),
        None,
    )?;
    let manifest_path = target.install_dir.join("chum-manifest.toml");
    let mut manifest = read_manifest_at(&target.install_dir)?;

    manifest.runtime.env.insert(key.clone(), value);

    write_manifest_at(&manifest_path, &manifest)?;

    output::emit_env_set(&target.name, &target.version, &key, common.json);
    Ok(())
}

async fn unset(common: EnvCommonArgs, key: String) -> Result<(), UserFacingError> {
    if !is_valid_env_key(&key) {
        return Err(UserFacingError::EnvKeyInvalid { key });
    }

    let target = resolve_lifecycle_target(
        &common.name,
        common.version.as_deref(),
        common.root.clone(),
        None,
    )?;
    let manifest_path = target.install_dir.join("chum-manifest.toml");
    let mut manifest = read_manifest_at(&target.install_dir)?;

    let was_set = manifest.runtime.env.remove(&key).is_some();
    if was_set {
        write_manifest_at(&manifest_path, &manifest)?;
    }

    output::emit_env_unset(&target.name, &target.version, &key, was_set, common.json);
    Ok(())
}

/// `^[A-Za-z_][A-Za-z0-9_]*$` — POSIX-ish env-var-name shape.
fn is_valid_env_key(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let mut chars = key.chars();
    let first = chars.next().expect("len > 0 checked above");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn read_manifest_at(install_dir: &std::path::Path) -> Result<chum_core::Manifest, UserFacingError> {
    let path = install_dir.join("chum-manifest.toml");
    let text = std::fs::read_to_string(&path).map_err(|_| UserFacingError::ManifestMissing {
        install_dir: install_dir.to_path_buf(),
    })?;
    chum_core::parse_and_validate(&text).map_err(UserFacingError::Manifest)
}

fn write_manifest_at(
    manifest_path: &std::path::Path,
    manifest: &chum_core::Manifest,
) -> Result<(), UserFacingError> {
    let serialized = toml::to_string(manifest).map_err(|e| UserFacingError::EnvUpdateFailed {
        path: manifest_path.to_path_buf(),
        reason: format!("serialise: {e}"),
    })?;
    std::fs::write(manifest_path, serialized).map_err(|e| UserFacingError::EnvUpdateFailed {
        path: manifest_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    Ok(())
}
