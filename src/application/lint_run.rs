//! The main use case: find the files under a set of paths, lint each one and
//! summarize the run.

use std::path::Path;

use crate::domain::lint::Linter;
use crate::domain::{FileResult, Summary, summarize};

use super::ports::{SourceReader, TargetFinder};

/// A target that vanished or became unreadable after discovery.
#[derive(Debug, thiserror::Error)]
#[error("cannot read {path}: {source}")]
pub struct ReadError {
    path: String,
    source: std::io::Error,
}

/// What a run found.
#[derive(Debug)]
pub struct Outcome {
    /// One per file that could be read, in discovery order.
    pub results: Vec<FileResult>,
    /// Files that could not be read. They are skipped rather than ending the
    /// run, but they still fail it.
    pub unreadable: Vec<ReadError>,
    pub summary: Summary,
}

impl Outcome {
    /// Whether the run should fail: any file with an error, or any file that
    /// could not be read. Warnings alone pass.
    pub fn failed(&self) -> bool {
        self.summary.files_with_errors > 0 || !self.unreadable.is_empty()
    }
}

/// Lints every file a [`TargetFinder`] turns up, reading each through a
/// [`SourceReader`].
pub struct LintRun<F, R> {
    finder: F,
    reader: R,
    linter: Linter,
}

impl<F: TargetFinder, R: SourceReader> LintRun<F, R> {
    pub fn new(finder: F, reader: R, linter: Linter) -> Self {
        LintRun {
            finder,
            reader,
            linter,
        }
    }

    /// Runs the lint. Only discovery can fail the run outright; a file that
    /// cannot be read is recorded in [`Outcome::unreadable`] and skipped.
    pub fn execute(&self, paths: &[String], excludes: &[String]) -> Result<Outcome, F::Error> {
        let targets = self.finder.find(paths, excludes)?;
        let mut results = Vec::with_capacity(targets.len());
        // A file can vanish between discovery and linting: skip it, but still fail.
        let mut unreadable = Vec::new();
        for t in &targets {
            match self.reader.read(Path::new(&t.path)) {
                Ok(src) => results.push(self.linter.check(t, &src)),
                Err(source) => unreadable.push(ReadError {
                    path: t.path.clone(),
                    source,
                }),
            }
        }
        let summary = summarize(&results);
        Ok(Outcome {
            results,
            unreadable,
            summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;

    use super::*;
    use crate::application::PathProbe;
    use crate::domain::{Config, Kind, Target};

    /// Finds a fixed list of skills, whatever it is asked.
    struct Fixed(Vec<&'static str>);

    #[derive(Debug, thiserror::Error)]
    #[error("never")]
    struct Never;

    impl TargetFinder for Fixed {
        type Error = Never;
        fn find(&self, _: &[String], _: &[String]) -> Result<Vec<Target>, Never> {
            Ok(self
                .0
                .iter()
                .map(|p| Target {
                    path: p.to_string(),
                    kind: Kind::Skill,
                    root: PathBuf::from("/repo"),
                })
                .collect())
        }
    }

    /// Serves files from memory; anything else is not found.
    struct Memory(HashMap<&'static str, &'static str>);

    impl SourceReader for Memory {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.0
                .get(path.to_str().unwrap())
                .map(|s| s.as_bytes().to_vec())
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }
    }

    struct Nothing;

    impl PathProbe for Nothing {
        fn exists(&self, _: &Path) -> bool {
            false
        }
    }

    fn run(found: Vec<&'static str>, files: &[(&'static str, &'static str)]) -> Outcome {
        let linter = Linter::new(Config::default(), PathBuf::from("/repo"), Box::new(Nothing));
        LintRun::new(
            Fixed(found),
            Memory(files.iter().copied().collect()),
            linter,
        )
        .execute(&[], &[])
        .unwrap()
    }

    const VALID: &str = "---\nname: ok\ndescription: Use this skill when a test needs one.\n---\n";

    #[test]
    fn clean_files_pass() {
        // Given
        let found = vec!["/repo/ok/SKILL.md"];
        let files = [("/repo/ok/SKILL.md", VALID)];

        // When
        let out = run(found, &files);

        // Then
        assert_eq!(out.results.len(), 1);
        assert!(out.results[0].findings.is_empty());
        assert!(!out.failed());
    }

    #[test]
    fn unreadable_files_are_skipped_but_fail_the_run() {
        // Given
        let found = vec!["/repo/ok/SKILL.md", "/repo/gone/SKILL.md"];
        let files = [("/repo/ok/SKILL.md", VALID)];

        // When
        let out = run(found, &files);

        // Then
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.unreadable.len(), 1);
        assert!(
            out.unreadable[0]
                .to_string()
                .starts_with("cannot read /repo/gone/SKILL.md: ")
        );
        assert_eq!(out.summary.files, 1);
        assert!(out.failed());
    }
}
