//! The local filesystem, behind the ports that read sources and probe paths.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::application::{PathProbe, SourceReader};

/// Reads files and checks paths on the machine lintmatter runs on.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFs;

/// Far above any real instruction file; it only bounds what a hostile tree
/// can make a run hold in memory.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

impl SourceReader for LocalFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        // A FIFO or device named like an instruction file would block or never
        // end, so only regular files are read.
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        let mut buf = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf)?;
        if buf.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::other("file is larger than 4 MiB"));
        }
        Ok(buf)
    }
}

impl PathProbe for LocalFs {
    fn exists(&self, root: &Path, path: &Path) -> bool {
        // The rule checks containment lexically, but a symlink inside the tree
        // can still lead out of it. Resolve the deepest part of the path that
        // exists: if that lands outside the root, answer "exists" whatever is
        // beyond it, so the answer carries nothing about the host.
        let Ok(root) = std::fs::canonicalize(root) else {
            return std::fs::metadata(path).is_ok();
        };
        match path.ancestors().find_map(|a| std::fs::canonicalize(a).ok()) {
            Some(real) if !real.starts_with(&root) => true,
            _ => std::fs::metadata(path).is_ok(),
        }
    }
}
