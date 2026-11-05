//! lintmatter lints agent instruction files (AGENTS.md and SKILL.md) for
//! front-matter correctness and token budget overruns.
//!
//! The crate is laid out as ports and adapters (hexagonal architecture), with
//! dependencies pointing inward only:
//!
//! - [`domain`] is the pure core: the file model, front-matter parsing, token
//!   estimation, the rules and the scoring. It performs no I/O; the one thing
//!   a rule must ask of the outside world, whether a path exists, goes through
//!   the [`domain::PathProbe`] port.
//! - [`application`] holds the use cases -- merging settings, running a lint
//!   over a set of paths -- and the [`application::ports`] they drive.
//! - [`adapters`] connects those to the real world: the command line, the
//!   filesystem, the `.lintmatter.yaml` file and the text and JSON reports.
//!   Each report owns its output format; the domain types carry no
//!   serialization attributes.
//!
//! `tests/architecture.rs` parses the sources and enforces the direction. `src/main.rs` is a thin
//! wrapper over [`adapters::cli::run`]. The library exists so the other crate
//! in the workspace, `lintmatter-web`, can read settings such as the default
//! token budgets from here rather than restating numbers that would then
//! drift.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod utils;
