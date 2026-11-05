use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::tokens::Counter;
use crate::domain::{Config, FileResult, Finding, Kind, PathProbe, Target};

use super::Linter;

pub(super) fn write_skill(skill_name: &str, src: &str) -> (tempfile::TempDir, Target) {
    let base = tempfile::tempdir().unwrap();
    let dir = base.path().join(skill_name);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("SKILL.md");
    fs::write(&path, src).unwrap();
    let target = Target {
        path: path.to_string_lossy().to_string(),
        kind: Kind::Skill,
        root: base.path().to_path_buf(),
    };
    (base, target)
}

pub(super) fn write_file(dir: &Path, name: &str, src: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    path.to_string_lossy().to_string()
}

pub(super) fn agents_target(dir: &Path, src: &str) -> Target {
    agents_target_under(dir, dir, src)
}

/// An AGENTS.md written into `dir`, but discovered under walk root `root`.
/// Separate from `agents_target` so a fixture can sit below its root, which
/// is what makes a `../` reference back into the tree distinguishable from
/// one that escapes it.
pub(super) fn agents_target_under(root: &Path, dir: &Path, src: &str) -> Target {
    Target {
        path: write_file(dir, "AGENTS.md", src),
        kind: Kind::Agents,
        root: root.to_path_buf(),
    }
}

pub(super) fn rule_ids(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.rule).collect()
}

/// Every budget off, so rule tests see only the rule under test. Spelled
/// out rather than derived from Config::default(), which now carries the
/// real budgets -- keep it explicit.
pub(super) fn generous_config() -> Config {
    Config {
        max_agents_tokens: 0,
        max_skill_tokens: 0,
        max_skill_name_tokens: 0,
        max_skill_description_tokens: 0,
        disabled: vec![],
        strict: false,
    }
}

pub(super) const VALID_SKILL: &str = "---\nname: valid-skill\ndescription: Use this skill when you need a fixture that passes every rule.\n---\n\n# Valid skill\n\nBody content.\n";

/// A counter reporting a fixed number, so a score test can sit exactly on
/// a budget boundary instead of depending on the estimator's heuristics.
pub(super) struct Fixed(pub(super) usize);

impl Counter for Fixed {
    fn count(&self, _: &str) -> usize {
        self.0
    }
}

/// Answers from the real filesystem, standing in for the filesystem adapter
/// so the engine's tests can lint fixtures written to a temp directory.
pub(crate) struct Disk;

impl PathProbe for Disk {
    fn exists(&self, _: &Path, path: &Path) -> bool {
        fs::metadata(path).is_ok()
    }
}

/// A linter over the real filesystem, rooted at the process's working
/// directory the way the CLI roots it.
pub(crate) fn linter(cfg: Config) -> Linter {
    linter_in(cfg, std::env::current_dir().unwrap_or_default())
}

/// A linter over the real filesystem with an explicit working directory.
pub(crate) fn linter_in(cfg: Config, cwd: PathBuf) -> Linter {
    Linter::new(cfg, cwd, Box::new(Disk))
}

/// Reading is the application layer's job, not the engine's; this lets the
/// engine's tests lint a fixture file in one call all the same.
pub(crate) trait ReadFile {
    fn file(&self, t: &Target) -> io::Result<FileResult>;
}

impl ReadFile for Linter {
    fn file(&self, t: &Target) -> io::Result<FileResult> {
        Ok(self.check(t, &fs::read(&t.path)?))
    }
}
