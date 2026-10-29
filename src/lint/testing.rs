//! Fixtures shared by the rule, engine and score tests.

use std::fs;
use std::path::Path;

use crate::model::{Budgets, FileResult, Finding, Kind, Target};
use crate::tokens::Counter;

use super::{LintOptions, Linter};

pub const VALID_SKILL: &str = "---\nname: valid-skill\ndescription: Use this skill when you need a fixture that passes every rule.\n---\n\n# Valid skill\n\nBody content.\n";

/// A SKILL.md in a directory named `skill_name`, under a fresh walk root.
pub fn write_skill(skill_name: &str, src: &str) -> (tempfile::TempDir, Target) {
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

pub fn write_file(dir: &Path, name: &str, src: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    path.to_string_lossy().to_string()
}

pub fn agents_target(dir: &Path, src: &str) -> Target {
    agents_target_under(dir, dir, src)
}

/// An AGENTS.md written into `dir`, but discovered under walk root `root`.
/// Separate from `agents_target` so a fixture can sit below its root, which
/// is what makes a `../` reference back into the tree distinguishable from
/// one that escapes it.
pub fn agents_target_under(root: &Path, dir: &Path, src: &str) -> Target {
    Target {
        path: write_file(dir, "AGENTS.md", src),
        kind: Kind::Agents,
        root: root.to_path_buf(),
    }
}

pub fn rule_ids(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.rule).collect()
}

/// Every budget off, so rule tests see only the rule under test. Spelled
/// out rather than derived from `LintOptions::default()`, which carries the
/// real budgets.
pub fn generous() -> LintOptions {
    LintOptions {
        budgets: Budgets {
            agents: 0,
            skill: 0,
            skill_name: 0,
            skill_description: 0,
        },
        disabled: vec![],
    }
}

/// A linter whose working directory is the test process's, which no fixture
/// sits directly in.
pub fn linter(opts: LintOptions) -> Linter {
    Linter::new(opts, std::env::current_dir().unwrap())
}

pub fn lint(opts: LintOptions, target: &Target) -> FileResult {
    linter(opts).file(target).unwrap()
}

/// Lints each `(case name, skill directory, source)` with every budget off
/// and asserts the exact rule ids it produces, in order.
pub fn assert_skill_cases(cases: Vec<(&str, &str, String, Vec<&str>)>) {
    for (name, skill_dir_name, src, want_rules) in cases {
        let (_base, target) = write_skill(skill_dir_name, &src);
        let res = lint(generous(), &target);
        assert_eq!(rule_ids(&res.findings), want_rules, "{name}");
    }
}

/// A counter reporting a fixed number, so a test can sit exactly on a
/// budget boundary instead of depending on the estimator's heuristics.
pub struct Fixed(pub usize);

impl Counter for Fixed {
    fn count(&self, _: &str) -> usize {
        self.0
    }
}
