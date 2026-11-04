//! The pure core: what an agent instruction file is, how it is parsed and
//! measured, and the rules that judge it. Nothing here touches the
//! filesystem, the environment or the command line.

pub mod config;
pub mod document;
pub mod fence;
pub mod finding;
pub mod kind;
pub mod lint;
pub mod ports;
pub mod summary;
pub mod target;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod tokens;

pub use config::Config;
pub use document::Document;
pub use finding::{Counts, FileResult, Finding, Severity};
pub use kind::{Kind, kind_for};
pub use ports::PathProbe;
pub use summary::{Summary, summarize};
pub use target::Target;
