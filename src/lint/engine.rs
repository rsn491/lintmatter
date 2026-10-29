//! Runs the registered rules over one file.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::model::{Budgets, Counts, FileResult, Kind, Target};
use crate::source::parse::Document;
use crate::tokens::{self, Counter};

use super::context::FileContext;
use super::rules;
use super::score::score;
use super::sink::{FindingSink, Tally};

/// What a run checks for. How the results are then reported -- `--strict`,
/// `--quiet`, the format -- is the caller's business, not the linter's.
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub budgets: Budgets<i64>,
    /// Rule ids to skip.
    pub disabled: Vec<String>,
}

impl Default for LintOptions {
    fn default() -> Self {
        LintOptions {
            budgets: Budgets::DEFAULT,
            disabled: Vec::new(),
        }
    }
}

/// Applies the registered rules using a token [`Counter`].
pub struct Linter {
    budgets: Budgets<i64>,
    disabled: HashSet<String>,
    counter: Box<dyn Counter>,
    /// Passed in rather than read from the process, so a verdict cannot
    /// depend on where the binary happened to be run, and tests can pin it.
    cwd: PathBuf,
}

impl Linter {
    pub fn new(opts: LintOptions, cwd: PathBuf) -> Self {
        Linter {
            budgets: opts.budgets,
            disabled: opts.disabled.into_iter().collect(),
            counter: Box::new(tokens::Estimator::new()),
            cwd,
        }
    }

    pub fn with_counter(mut self, counter: Box<dyn Counter>) -> Self {
        self.counter = counter;
        self
    }

    /// Reads and lints one target.
    pub fn file(&self, t: &Target) -> Result<FileResult, String> {
        let src = std::fs::read(&t.path).map_err(|e| format!("cannot read {}: {e}", t.path))?;
        Ok(self.check(t, &src))
    }

    /// Lints already-read file contents.
    ///
    /// Rules run in registry order, so findings come out in that order without
    /// a sort: output never depends on the order checks happen to run in.
    pub fn check(&self, t: &Target, src: &[u8]) -> FileResult {
        let doc = Document::parse(src);

        // Only a skill has a name and description to be judged, so only a
        // skill carries those token counts. Measured once here rather than as
        // a side effect of validating them, so rules only compare.
        let fm = if t.kind == Kind::Skill {
            doc.mapping()
        } else {
            None
        };
        let name = fm.and_then(|f| non_empty(f.string("name")));
        let description = fm.and_then(|f| non_empty(f.string("description")));
        let tokens = Counts {
            content: self.counter.count(&doc.body),
            name: name.as_deref().map_or(0, |s| self.counter.count(s)),
            description: description.as_deref().map_or(0, |s| self.counter.count(s)),
        };

        let mut findings = Vec::new();
        let mut tally = Tally::default();
        {
            let ctx = FileContext {
                target: t,
                doc: &doc,
                tokens: &tokens,
                budgets: &self.budgets,
                cwd: &self.cwd,
                name,
                description,
            };
            for rule in rules::all() {
                let meta = rule.meta();
                if !meta.applies_to(t.kind) || self.disabled.contains(meta.id) {
                    continue;
                }
                let mut sink = FindingSink::new(&mut findings, &mut tally, meta);
                rule.check(&ctx, &mut sink);
            }
        }

        FileResult {
            path: t.path.clone(),
            kind: t.kind,
            score: score(&tally),
            tokens,
            findings,
        }
    }
}

/// Trims a front-matter scalar, discarding it when nothing is left.
fn non_empty(raw: Option<String>) -> Option<String> {
    let s = raw?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::testing::*;

    #[test]
    fn findings_come_out_in_registry_order() {
        assert_skill_cases(vec![
            (
                "valid skill has no findings",
                "valid-skill",
                VALID_SKILL.to_string(),
                vec![],
            ),
            (
                "several problems are all reported",
                "multi",
                "---\nname: Multi_Skill\nextra: true\n---\nBody.\n".to_string(),
                vec![
                    "frontmatter.unknown-key",
                    "name.format",
                    "name.dir-mismatch",
                    "description.required",
                ],
            ),
        ]);
    }

    #[test]
    fn agents_frontmatter_is_not_validated() {
        let base = tempfile::tempdir().unwrap();
        let src =
            "---\ntitle: Project instructions\nowner: infra\n---\n\n# Instructions\n\nBody.\n";
        let target = agents_target(base.path(), src);

        let res = lint(generous(), &target);
        assert!(res.findings.is_empty());
        assert!(res.tokens.content > 0);
    }

    #[test]
    fn agents_frontmatter_excluded_from_content() {
        let base = tempfile::tempdir().unwrap();
        let fm_src =
            "---\ntitle: A fairly wordy front matter block that costs real tokens\n---\nBody.\n";
        let with_fm = agents_target(base.path(), fm_src);
        let base2 = tempfile::tempdir().unwrap();
        let bare = agents_target(base2.path(), "Body.\n");

        let checker = linter(generous());
        let with_res = checker.file(&with_fm).unwrap();
        let bare_res = checker.file(&bare).unwrap();
        assert_eq!(with_res.tokens.content, bare_res.tokens.content);
    }

    #[test]
    fn with_counter_substitutes_the_token_counter() {
        let (_base, target) = write_skill("counted", VALID_SKILL);

        let mut opts = generous();
        opts.budgets.skill = 10;
        let res = linter(opts.clone())
            .with_counter(Box::new(Fixed(11)))
            .file(&target)
            .unwrap();
        assert_eq!(res.tokens.content, 11);
        assert!(res.findings.iter().any(|f| f.rule == "tokens.content"));

        let res = linter(opts)
            .with_counter(Box::new(Fixed(9)))
            .file(&target)
            .unwrap();
        assert_eq!(res.tokens.content, 9);
        assert!(!res.findings.iter().any(|f| f.rule == "tokens.content"));
    }

    #[test]
    fn default_options_enforce_budgets() {
        assert_eq!(LintOptions::default().budgets, Budgets::DEFAULT);

        // The point of the hand-written Default: reaching for it must not
        // hand back a linter that checks nothing.
        let base = tempfile::tempdir().unwrap();
        let body = "some filler prose to spend tokens on. ".repeat(500);
        let target = agents_target(base.path(), &body);
        let res = linter(LintOptions::default()).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec!["tokens.content"]);
    }

    #[test]
    fn disable_skips_rules() {
        let (_base, target) = write_skill(
            "mismatched-dir",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
        );

        let mut opts = generous();
        opts.disabled = vec!["name.format".to_string(), "name.dir-mismatch".to_string()];
        let res = linter(opts).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }

    #[test]
    fn findings_carry_file_and_line() {
        let (_base, target) = write_skill(
            "lines",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
        );

        let res = lint(generous(), &target);
        assert_eq!(res.path, target.path);
        for f in &res.findings {
            if f.rule == "name.format" {
                assert_eq!(f.line, 2);
            }
        }
    }

    #[test]
    fn missing_file_is_an_error() {
        let base = tempfile::tempdir().unwrap();
        let target = Target {
            path: base.path().join("SKILL.md").to_string_lossy().to_string(),
            kind: Kind::Skill,
            root: base.path().to_path_buf(),
        };
        assert!(linter(generous()).file(&target).is_err());
    }
}
