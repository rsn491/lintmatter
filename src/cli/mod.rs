//! The command line: parses flags, merges them with the config file, runs
//! discovery and the linter, and hands the results to a reporter.
//!
//! [`run`] takes its writers as arguments so tests drive the whole CLI
//! in-process.

mod args;
mod run;
mod usage;

pub use run::run;

/// No errors were found; warnings alone still exit OK.
pub const EXIT_OK: i32 = 0;
/// At least one error-severity finding was reported.
pub const EXIT_FINDINGS: i32 = 1;
/// The run could not happen: bad flags or unreadable files.
pub const EXIT_USAGE: i32 = 2;

/// The reported build version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
