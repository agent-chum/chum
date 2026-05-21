//! Subcommand implementations.
//!
//! Each module owns one subcommand's pipeline. The top-level `main.rs`
//! dispatches into these by matching on the clap `Command` enum.
//!
//! Shared helpers (like [`resolve_root`]) live here so individual
//! subcommands stay focused on their own pipeline.

use std::path::PathBuf;

use crate::error::UserFacingError;

pub mod install;
pub mod list;

/// Resolve the CHUM root directory for this invocation.
///
/// Returns `arg` verbatim if a `--root` override was supplied. Otherwise
/// calls [`chum_install::chum_home`] and maps the "no env vars set" case
/// (manifesting as `InstallError::Io` with `ErrorKind::NotFound`) into
/// the clean [`UserFacingError::ChumHomeUnresolved`] code, so callers do
/// not leak the `install_io` code for what is really a configuration
/// gap.
pub(crate) fn resolve_root(arg: Option<PathBuf>) -> Result<PathBuf, UserFacingError> {
    if let Some(r) = arg {
        return Ok(r);
    }
    chum_install::chum_home().map_err(|e| match e {
        chum_install::InstallError::Io(io)
            if io.kind() == std::io::ErrorKind::NotFound =>
        {
            UserFacingError::ChumHomeUnresolved
        }
        other => UserFacingError::Install(other),
    })
}
