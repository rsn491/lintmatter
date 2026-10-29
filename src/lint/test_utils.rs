use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::discover::{Kind, Target};
use crate::tokens::Counter;

use super::finding::Finding;

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
