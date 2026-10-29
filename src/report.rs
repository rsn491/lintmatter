//! Renders lint results for humans and for machines.

mod json;
mod reporter;
mod summary;
#[cfg(test)]
mod test_support;
mod text;

pub use json::{JsonReporter, SCHEMA_VERSION};
pub use reporter::Report;
pub use summary::{Summary, summarize};
pub use text::TextReporter;
