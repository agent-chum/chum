//! Subcommand implementations.
//!
//! Each module owns one subcommand's pipeline. The top-level `main.rs`
//! dispatches into these by matching on the clap `Command` enum.

pub mod install;
