//! The driven ports: what the use cases need from the outside world, stated
//! as traits so the filesystem adapter can be swapped for an in-memory one.

use std::io;
use std::path::Path;

use crate::domain::Target;

/// Owned by the domain, because a rule needs it; listed here with the rest.
pub use crate::domain::PathProbe;

/// Reads a target's raw bytes.
pub trait SourceReader {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
}

/// Turns the paths a run was given into the files to lint.
pub trait TargetFinder {
    type Error: std::error::Error;

    /// Returns the targets under `paths`, skipping any matching an `excludes`
    /// glob, deduplicated and in a stable order.
    fn find(&self, paths: &[String], excludes: &[String]) -> Result<Vec<Target>, Self::Error>;
}
