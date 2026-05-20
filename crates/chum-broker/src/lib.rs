//! `chum-broker` — permission and secrets broker between agents/clients and MCP servers.
//!
//! Per the architecture invariants in `CLAUDE.md`:
//! - This crate **gates all agent ↔ MCP server access**. No direct
//!   passthrough; every capability use is mediated here.
//! - Per-tool grants, scoped secrets, and path allowlists live in this
//!   crate.
//!
//! This is the v0.0.1 scaffold — only `placeholder()` exists. The grant
//! model, secret-store integration, and proxy layer arrive in v0.2.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Placeholder export so the crate compiles before real types land.
///
/// Will be removed in the first feature commit that introduces an actual
/// grant or proxy type.
pub fn placeholder() {}
