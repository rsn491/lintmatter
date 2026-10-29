//! Applies lintmatter's rules to agent instruction files.

mod context;
mod finding;
mod ids;
mod linter;
pub mod rule;
pub mod rules;
mod score;
mod sink;
#[cfg(test)]
mod test_support;

pub use context::FileContext;
pub use finding::{Counts, FileResult, Finding, Severity};
pub use ids::*;
pub use linter::{Linter, ReadError};
pub use sink::FindingSink;
