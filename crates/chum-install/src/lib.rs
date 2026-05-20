//! `chum-install` — install-time I/O for CHUM manifests.
//!
//! Pure async transformation: [`chum_core::Manifest`] → [`InstalledArtifact`].
//! Filesystem, network, and subprocess work all live here. The daemon
//! (`chum-daemon`) and the registry (`chum-registry`) consume the result;
//! `chum-install` does not persist anything itself.
//!
//! Per chum-core's no-I/O invariant, this crate OWNS install-side I/O.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod fetcher;
pub mod install;
pub mod paths;
pub mod sources;

pub use error::InstallError;
pub use fetcher::{Fetcher, ReqwestFetcher};
pub use install::{install, InstallConfig, InstalledArtifact, SourceKind};
pub use paths::chum_home;
