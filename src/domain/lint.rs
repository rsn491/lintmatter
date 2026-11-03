//! Applies lintmatter's rules to agent instruction files.

mod context;
mod ids;
mod linter;
pub mod rule;
pub mod rules;
mod score;
mod sink;
#[cfg(test)]
mod test_utils;

pub use context::{FileContext, PathProbe};
pub use ids::*;
pub use linter::Linter;
pub use sink::FindingSink;
