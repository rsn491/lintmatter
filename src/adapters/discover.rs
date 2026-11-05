//! Turns command-line paths into the set of agent instruction files to lint.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use glob::Pattern;

use crate::application::TargetFinder;
use crate::domain::{Kind, Target, kind_for};
use crate::utils::to_slash;

/// Walks the local filesystem for agent instruction files.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsDiscoverer;

impl TargetFinder for FsDiscoverer {
    type Error = DiscoverError;

    fn find(&self, paths: &[String], excludes: &[String]) -> Result<Vec<Target>, DiscoverError> {
        find(paths, excludes)
    }
}

/// Why discovery could not produce a target list.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("cannot read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} is not an AGENTS.md or SKILL.md", .0.display())]
    NotInstructionFile(PathBuf),
    #[error("{} is more than {MAX_DEPTH} directories deep: refusing to recurse further", .0.display())]
    TooDeep(PathBuf),
    #[error("invalid exclude pattern {pattern:?}: {source}")]
    BadExclude {
        pattern: String,
        source: glob::PatternError,
    },
}

fn read_err(path: &Path) -> impl FnOnce(io::Error) -> DiscoverError + '_ {
    move |source| DiscoverError::Read {
        path: path.to_path_buf(),
        source,
    }
}

/// Recursion limit, so a pathological layout cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

/// Never walked: holds dependencies and build output, whose instruction files
/// are not the user's to fix.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".next",
    ".venv",
    "build",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

/// Collects the files to lint from the given paths. A file argument is taken
/// as-is; a directory is walked recursively. Excludes are shell globs matched
/// against each entry's base name and its path relative to the walk root, and
/// a matching directory is pruned.
///
/// The result is deduplicated by absolute path and sorted, so output is
/// stable across runs.
pub fn find(paths: &[String], excludes: &[String]) -> Result<Vec<Target>, DiscoverError> {
    let mut d = Discoverer::new(compile_globs(excludes)?);
    d.collect(paths)?;
    Ok(d.finish())
}

/// Accumulates targets across a walk.
///
/// The recursion used to thread a `&mut impl FnMut(&Path, Kind, &Path)` closure
/// down every level just to push onto two locals; holding them here says the
/// same thing without passing a callback through each frame.
struct Discoverer {
    patterns: Vec<Pattern>,
    /// Absolute paths already added, so naming a file twice -- or naming it
    /// and the directory holding it -- yields one target.
    seen: HashSet<PathBuf>,
    targets: Vec<Target>,
}

impl Discoverer {
    fn new(patterns: Vec<Pattern>) -> Self {
        Discoverer {
            patterns,
            seen: HashSet::new(),
            targets: Vec::new(),
        }
    }

    /// Adds every target under `paths`.
    fn collect(&mut self, paths: &[String]) -> Result<(), DiscoverError> {
        paths.iter().try_for_each(|path| {
            let path = Path::new(path);
            let meta = fs::metadata(path).map_err(read_err(path))?;
            if meta.is_dir() {
                return self.walk(path, path, 0);
            }
            // A file named outright has no walk root, so it stands as its own:
            // its directory is the tree we were pointed at.
            let root = path.parent().unwrap_or(Path::new("."));
            let kind = kind_for(path)
                .ok_or_else(|| DiscoverError::NotInstructionFile(path.to_path_buf()))?;
            self.add(path, kind, root);
            Ok(())
        })
    }

    /// Returns the collected targets, sorted by path.
    fn finish(mut self) -> Vec<Target> {
        self.targets.sort_by(|a, b| a.path.cmp(&b.path));
        self.targets
    }

    fn add(&mut self, path: &Path, kind: Kind, root: &Path) {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if self.seen.insert(abs) {
            self.targets.push(Target {
                path: to_slash(path),
                kind,
                root: root.to_path_buf(),
            });
        }
    }

    fn walk(&mut self, root: &Path, dir: &Path, depth: usize) -> Result<(), DiscoverError> {
        if depth > MAX_DEPTH {
            return Err(DiscoverError::TooDeep(dir.to_path_buf()));
        }
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(read_err(dir))?
            .collect::<Result<_, _>>()
            .map_err(read_err(dir))?;
        entries.sort_by_key(|e| e.file_name());

        entries.into_iter().try_for_each(|entry| {
            let path = entry.path();
            let file_type = entry.file_type().map_err(read_err(&path))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let rel = || to_slash(path.strip_prefix(root).unwrap_or(&path));

            if file_type.is_dir() {
                if SKIP_DIRS.contains(&name.as_ref()) || self.excluded(&name, &rel) {
                    return Ok(());
                }
                return self.walk(root, &path, depth + 1);
            }
            if let Some(kind) = kind_for(&path)
                && !self.excluded(&name, &rel)
            {
                self.add(&path, kind, root);
            }
            Ok(())
        })
    }

    /// Whether an entry matches any `--exclude` glob, by base name or by its
    /// path relative to the walk root. `rel` is a closure so the relative path
    /// is only built when there are patterns to match it against.
    fn excluded(&self, name: &str, rel: &dyn Fn() -> String) -> bool {
        !self.patterns.is_empty() && matches_any(&self.patterns, name, &rel())
    }
}

fn matches_any(patterns: &[Pattern], name: &str, rel: &str) -> bool {
    patterns.iter().any(|p| p.matches(name) || p.matches(rel))
}

/// Rejects malformed patterns up front rather than silently matching nothing
/// on every path. The message names no flag because an exclude can come from
/// `--exclude` or from the config file's `exclude:`.
fn compile_globs(globs: &[String]) -> Result<Vec<Pattern>, DiscoverError> {
    globs
        .iter()
        .map(|g| {
            Pattern::new(g).map_err(|source| DiscoverError::BadExclude {
                pattern: g.clone(),
                source,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, contents) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    fn rel_paths(root: &Path, targets: &[Target]) -> Vec<String> {
        targets
            .iter()
            .map(|t| {
                let rel = Path::new(&t.path).strip_prefix(root).unwrap();
                format!("{} ({})", to_slash(rel), t.kind)
            })
            .collect()
    }

    #[test]
    fn find_walks_recursively() {
        // Given
        let dir = tree(&[
            ("AGENTS.md", "root instructions"),
            ("README.md", "not an instruction file"),
            ("docs/AGENTS.md", "nested instructions"),
            ("skills/alpha/SKILL.md", "alpha"),
            ("skills/beta/skill.md", "beta, lowercase"),
            ("skills/gamma/SKILLS.md", "gamma, plural, not matched"),
            ("node_modules/pkg/SKILL.md", "dependency skill"),
            (".git/hooks/AGENTS.md", "git internals"),
            ("vendor/dep/AGENTS.md", "vendored"),
            ("dist/AGENTS.md", "build output"),
            ("skills/alpha/reference/doc.md", "supporting doc"),
        ]);
        let want = vec![
            "AGENTS.md (agents)".to_string(),
            "docs/AGENTS.md (agents)".to_string(),
            "skills/alpha/SKILL.md (skill)".to_string(),
            "skills/beta/skill.md (skill)".to_string(),
        ];

        // When
        let targets = find(&[dir.path().to_string_lossy().to_string()], &[]).unwrap();

        // Then
        assert_eq!(rel_paths(dir.path(), &targets), want);
    }

    #[test]
    fn find_excludes() {
        // Given
        let dir = tree(&[
            ("AGENTS.md", "root"),
            ("examples/AGENTS.md", "example"),
            ("skills/keep/SKILL.md", "keep"),
            ("skills/drop/SKILL.md", "drop"),
            ("fixtures/bad/SKILL.md", "fixture"),
        ]);
        let root = dir.path().to_string_lossy().to_string();

        let cases: &[(&[&str], &[&str])] = &[
            (
                &["examples"],
                &[
                    "AGENTS.md (agents)",
                    "fixtures/bad/SKILL.md (skill)",
                    "skills/drop/SKILL.md (skill)",
                    "skills/keep/SKILL.md (skill)",
                ],
            ),
            (
                &["skills/drop"],
                &[
                    "AGENTS.md (agents)",
                    "examples/AGENTS.md (agents)",
                    "fixtures/bad/SKILL.md (skill)",
                    "skills/keep/SKILL.md (skill)",
                ],
            ),
            (
                &["examples", "fixtures"],
                &[
                    "AGENTS.md (agents)",
                    "skills/drop/SKILL.md (skill)",
                    "skills/keep/SKILL.md (skill)",
                ],
            ),
            (
                &["AGENTS.md"],
                &[
                    "fixtures/bad/SKILL.md (skill)",
                    "skills/drop/SKILL.md (skill)",
                    "skills/keep/SKILL.md (skill)",
                ],
            ),
        ];
        for (excludes, want) in cases {
            let excludes: Vec<String> = excludes.iter().map(|s| s.to_string()).collect();
            let want: Vec<String> = want.iter().map(|s| s.to_string()).collect();

            // When
            let targets = find(std::slice::from_ref(&root), &excludes).unwrap();

            // Then
            assert_eq!(
                rel_paths(dir.path(), &targets),
                want,
                "excludes = {excludes:?}"
            );
        }
    }

    #[test]
    fn find_explicit_file() {
        // Given
        let dir = tree(&[
            ("nested/deep/SKILL.md", "a skill somewhere"),
            ("README.md", "not an instruction file"),
        ]);
        let file = dir.path().join("nested/deep/SKILL.md");
        let skipped = tree(&[("node_modules/pkg/SKILL.md", "dependency skill")]);
        let skipped_file = skipped.path().join("node_modules/pkg/SKILL.md");

        // When
        let targets = find(&[file.to_string_lossy().to_string()], &[]).unwrap();
        let skipped_targets = find(&[skipped_file.to_string_lossy().to_string()], &[]).unwrap();

        // Then
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].kind, Kind::Skill);
        assert_eq!(
            skipped_targets.len(),
            1,
            "an explicit file inside a skipped directory is still linted"
        );
    }

    #[test]
    fn find_errors() {
        // Given
        let dir = tree(&[("README.md", "not an instruction file")]);
        let not_instructions = dir.path().join("README.md").to_string_lossy().to_string();
        let missing = dir.path().join("nope").to_string_lossy().to_string();
        let root = dir.path().to_string_lossy().to_string();

        // When
        let not_instructions_err = find(&[not_instructions], &[]).unwrap_err().to_string();
        let missing_result = find(&[missing], &[]);
        let bad_exclude_err = find(&[root], &["[".to_string()]).unwrap_err().to_string();

        // Then
        assert!(
            not_instructions_err.contains("not an AGENTS.md or SKILL.md"),
            "{not_instructions_err}"
        );
        assert!(missing_result.is_err());
        assert!(
            bad_exclude_err.contains("invalid exclude"),
            "{bad_exclude_err}"
        );
    }

    #[test]
    fn find_stops_at_the_depth_limit() {
        // Given
        let dir = tempfile::tempdir().unwrap();
        let deep: PathBuf = (0..MAX_DEPTH + 5).map(|i| format!("d{i}")).collect();
        fs::create_dir_all(dir.path().join(&deep)).unwrap();
        fs::write(dir.path().join(&deep).join("AGENTS.md"), "too deep").unwrap();
        let ok_dir = tempfile::tempdir().unwrap();
        let shallow: PathBuf = (0..MAX_DEPTH - 1).map(|i| format!("d{i}")).collect();
        fs::create_dir_all(ok_dir.path().join(&shallow)).unwrap();
        fs::write(ok_dir.path().join(&shallow).join("AGENTS.md"), "deep").unwrap();

        // When
        let err = find(&[dir.path().to_string_lossy().to_string()], &[])
            .unwrap_err()
            .to_string();
        let targets = find(&[ok_dir.path().to_string_lossy().to_string()], &[]).unwrap();

        // Then
        assert!(err.contains("directories deep"), "{err}");
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn find_deduplicates() {
        // Given
        let dir = tree(&[("AGENTS.md", "root")]);
        let root = dir.path().to_string_lossy().to_string();
        let file = dir.path().join("AGENTS.md").to_string_lossy().to_string();

        // When
        let targets = find(&[root, file.clone(), file], &[]).unwrap();

        // Then
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn find_empty_result() {
        // Given
        let dir = tree(&[("README.md", "nothing to lint here")]);

        // When
        let targets = find(&[dir.path().to_string_lossy().to_string()], &[]).unwrap();

        // Then
        assert!(targets.is_empty());
    }
}
