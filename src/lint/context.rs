//! What a rule may look at for one file.

use std::path::{Path, PathBuf};

use super::finding::Counts;
use crate::config::Config;
use crate::discover::Target;
use crate::parse::{self, Document, ErrKind, Frontmatter};
use crate::utils::clean_path;

/// Everything a rule may look at for one file.
///
/// Rules read the working directory from here rather than from the process, so
/// a verdict cannot depend on where the binary happened to be run.
pub struct FileContext<'a> {
    pub target: &'a Target,
    pub doc: &'a Document,
    pub tokens: &'a Counts,
    pub cfg: &'a Config,
    cwd: &'a Path,
    name: Option<String>,
    description: Option<String>,
}

impl<'a> FileContext<'a> {
    pub(super) fn new(
        target: &'a Target,
        doc: &'a Document,
        tokens: &'a Counts,
        cfg: &'a Config,
        cwd: &'a Path,
        name: Option<String>,
        description: Option<String>,
    ) -> Self {
        FileContext {
            target,
            doc,
            tokens,
            cfg,
            cwd,
            name,
            description,
        }
    }

    /// The front matter, but only when it is usable: present, parsed, and a
    /// mapping. Rules that read keys go through this, so a malformed block
    /// short-circuits all of them the same way.
    pub fn frontmatter(&self) -> Option<&Frontmatter> {
        self.doc.mapping()
    }

    /// The skill's name: `Some` only when present and non-empty after
    /// trimming. That is what keeps `name.format` and friends quiet while
    /// `name.required` fires, without any rule knowing about the others.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The skill's description, under the same rule as [`Self::name`].
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The parse error, when it is of the given kind.
    pub fn error_of(&self, kind: ErrKind) -> Option<&parse::Error> {
        self.doc.error.as_ref().filter(|e| e.kind == kind)
    }

    /// Joins `path` onto the working directory when relative and lexically
    /// normalizes it, without touching the filesystem or resolving symlinks
    /// (mirroring Go's `filepath.Abs`).
    pub fn abs_path(&self, path: &Path) -> PathBuf {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        clean_path(&joined)
    }

    /// The name of the directory holding the skill file, or `None` when there
    /// is no meaningful directory to match the name against: a loose skill
    /// file in the working directory is named after the project, not the
    /// skill.
    pub fn skill_dir(&self) -> Option<String> {
        let abs = self.abs_path(Path::new(&self.target.path));
        let dir = abs.parent()?.to_path_buf();
        if dir == clean_path(self.cwd) {
            return None;
        }
        dir.file_name()?.to_str().map(str::to_string)
    }
}
