//! lintmatter lints agent instruction files (AGENTS.md and SKILL.md) for
//! front-matter correctness and token budget overruns.
//!
//! `src/main.rs` is a thin wrapper over [`cli::run`]. The library exists so
//! the other crate in the workspace, `lintmatter-web`, can read settings such as
//! the default token budgets from here rather than restating numbers that
//! would then drift.

pub mod cli;
pub mod config;
pub mod discover;
pub mod fence;
pub mod lint;
pub mod parse;
pub mod report;
pub mod tokens;
pub mod utils;
