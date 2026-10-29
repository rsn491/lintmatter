//! One struct per rule id, and the registry that orders them.

pub mod description;
pub mod frontmatter;
pub mod name;
pub mod references;
mod registry;
pub mod schema;
pub mod tokens;

pub use registry::all;
