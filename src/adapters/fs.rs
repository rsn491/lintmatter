//! The local filesystem, behind the ports that read sources and probe paths.

use std::io;
use std::path::Path;

use crate::application::{PathProbe, SourceReader};

/// Reads files and checks paths on the machine lintmatter runs on.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFs;

impl SourceReader for LocalFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

impl PathProbe for LocalFs {
    fn exists(&self, path: &Path) -> bool {
        std::fs::metadata(path).is_ok()
    }
}
