//! Applies lintmatter's rules to agent instruction files.
//!
//! [`Linter`] parses a file, measures its token counts, runs every rule in
//! [`rules::all`] over it through a [`FindingSink`], and scores the outcome.
//! A rule is a struct implementing [`Rule`]; everything lintmatter knows
//! about it without running it is its [`RuleMeta`].

mod context;
mod engine;
pub mod rule;
pub mod rules;
mod score;
mod sink;
#[cfg(test)]
mod testing;

use std::sync::LazyLock;

pub use context::FileContext;
pub use engine::{LintOptions, Linter};
pub use rule::{Part, Rule, RuleMeta};
pub use sink::FindingSink;

/// Every rule id lintmatter can emit, in report order. Derived from the registry,
/// so it cannot drift from the rules that actually run.
pub static RULES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| rules::all().iter().map(|r| r.meta().id).collect());
