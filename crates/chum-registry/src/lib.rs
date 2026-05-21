//! `chum-registry` — local install-record store for CHUM.
//!
//! Exclusively owns `<chum_home>/state.db`. Per the architecture
//! invariants in `CLAUDE.md`, this crate **persists but does not act**:
//! it never writes to `packages/` or `bin/`, never spawns processes,
//! never touches the network.
//!
//! The public surface:
//! - [`Registry`] — read/write handle, one per database file.
//! - [`RegistryArtifact`] — the row shape returned from reads.
//! - [`RegistryError`] — the error enum every method returns.
//! - [`CURRENT_SCHEMA_VERSION`] — the schema version this binary writes.
//! - [`chum_home`] — re-exported from `chum-install`; the registry does
//!   not duplicate root resolution.
//!
//! Quickstart:
//!
//! ```no_run
//! use chum_registry::{Registry, chum_home};
//!
//! let path = chum_home().expect("CHUM_HOME").join("state.db");
//! let registry = Registry::open(path).expect("open registry");
//! let rows = registry.list_all().expect("list");
//! # let _ = (registry, rows);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod registry;
mod schema;
mod types;

pub use chum_install::chum_home;
pub use error::RegistryError;
pub use registry::Registry;
pub use schema::CURRENT_SCHEMA_VERSION;
pub use types::{Grant, RegistryArtifact};
