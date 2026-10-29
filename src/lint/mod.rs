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
mod tests;

use std::sync::LazyLock;

pub use context::FileContext;
pub use finding::{Counts, FileResult, Finding, Severity};
pub use ids::*;
pub use linter::Linter;
pub use sink::FindingSink;

/// Every rule id lintmatter can emit, in report order. Derived from the registry,
/// so it cannot drift from the rules that actually run.
pub static RULES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| rules::all().iter().map(|r| r.id()).collect());
