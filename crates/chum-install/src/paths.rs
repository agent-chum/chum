//! CHUM root directory + per-package path resolution.
//!
//! The layout under the root is locked at v0.1:
//!
//! ```text
//! <root>/
//! ├── packages/<name>/<version>/    # one per installed package
//! ├── bin/                           # symlinks (install writes, daemon reads)
//! ├── state.db                       # chum-registry territory; never touched here
//! └── cache/downloads/               # in-flight binary fetches
//! ```

use std::path::{Path, PathBuf};

use crate::InstallError;

/// Resolve the CHUM root directory.
///
/// Resolution order:
///
/// 1. `$CHUM_HOME` if set (explicit override, used by tests and power users).
/// 2. `$XDG_DATA_HOME/chum/` if `$XDG_DATA_HOME` is set (XDG-compliant Linux
///    or anyone who has set the variable on macOS).
/// 3. `$HOME/.chum/` otherwise (the default on macOS).
///
/// # Errors
///
/// Returns [`InstallError::Io`] with [`std::io::ErrorKind::NotFound`] if
/// none of `$CHUM_HOME`, `$XDG_DATA_HOME`, or `$HOME` is set in the
/// process environment.
pub fn chum_home() -> Result<PathBuf, InstallError> {
    if let Ok(home) = std::env::var("CHUM_HOME") {
        return Ok(PathBuf::from(home));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("chum"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Ok(PathBuf::from(home).join(".chum"));
    }
    Err(InstallError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no $CHUM_HOME, $XDG_DATA_HOME, or $HOME in environment",
    )))
}

/// Per-package install directory: `<root>/packages/<name>/<version>/`.
pub fn package_dir(root: &Path, name: &str, version: &str) -> PathBuf {
    root.join("packages").join(name).join(version)
}

/// The shared `bin/` directory under the root. `chum-install` writes
/// symlinks here; `chum-daemon` reads them when starting servers.
pub fn bin_dir(root: &Path) -> PathBuf {
    root.join("bin")
}

/// The download cache directory: `<root>/cache/downloads/`.
pub fn cache_dir(root: &Path) -> PathBuf {
    root.join("cache").join("downloads")
}
