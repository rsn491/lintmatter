//! The one port the domain owns. A rule needs it while judging a file, and
//! the domain may not depend on `application`, so it cannot live with the
//! use cases' ports; `application::ports` re-exports it so every port can be
//! found in one place.

use std::path::Path;

/// Answers whether a path exists, so a rule can check a reference without the
/// domain touching the filesystem itself. The filesystem adapter provides the
/// real one; tests can substitute a fixed answer.
pub trait PathProbe: Sync {
    /// Whether `path`, a location under `root`, exists. A path that only
    /// reaches outside `root` through a symlink must come back `true`, so a
    /// reference can never tell the author what exists elsewhere on the host.
    fn exists(&self, root: &Path, path: &Path) -> bool;
}
