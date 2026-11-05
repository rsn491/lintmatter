//! A file chosen for linting.

use std::path::PathBuf;

use super::Kind;

/// A single file to lint.
#[derive(Debug, Clone)]
pub struct Target {
    pub path: String,
    pub kind: Kind,
    /// The tree this target was found under: the directory argument that
    /// produced it, or the file's own directory when named explicitly. Rules
    /// use it to tell a reference into the linted tree from one that escapes.
    pub root: PathBuf,
}
