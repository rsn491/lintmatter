//! Renders lint results for humans and for machines.

mod json;
mod reporter;
mod text;

pub use json::{JsonReporter, SCHEMA_VERSION};
pub use reporter::Report;
pub use text::TextReporter;
