//! Per-source-kind install handlers.
//!
//! Each submodule implements one [`chum_core::manifest::Source`]
//! variant. The top-level [`crate::install::install`] function (in a
//! later commit) dispatches into these by pattern matching.

pub mod local;
