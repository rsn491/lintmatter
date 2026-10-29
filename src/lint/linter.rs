use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::Config;
use crate::discover::{Kind, Target};
use crate::parse::Document;
use crate::tokens::{self, Counter};

use super::context::FileContext;
use super::finding::{Counts, FileResult};
use super::rules;
use super::score::score;
use super::sink::{FindingSink, Tally};

/// A target that vanished or became unreadable after discovery.
#[derive(Debug, thiserror::Error)]
#[error("cannot read {path}: {source}")]
pub struct ReadError {
    path: String,
    source: std::io::Error,
}

/// Applies [`Config`]'s rules using a token [`Counter`].
pub struct Linter {
    cfg: Config,
    counter: Box<dyn Counter>,
    disabled: HashSet<String>,
    /// Resolved once, at construction. Rules read this through FileContext
    /// instead of calling `std::env::current_dir` themselves.
    cwd: PathBuf,
}

impl Linter {
    pub fn new(cfg: Config) -> Self {
        let disabled = cfg.disabled.iter().cloned().collect();
        let linter = Linter {
            cfg,
            counter: Box::new(tokens::Estimator::new()),
            disabled,
            cwd: PathBuf::new(),
        };
        linter.with_cwd(std::env::current_dir().unwrap_or_default())
    }

    pub fn with_counter(mut self, counter: Box<dyn Counter>) -> Self {
        self.counter = counter;
        self
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = cwd;
        self
    }

    /// Reads and lints one target.
    pub fn file(&self, t: &Target) -> Result<FileResult, ReadError> {
        let src = std::fs::read(&t.path).map_err(|source| ReadError {
            path: t.path.clone(),
            source,
        })?;
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
        // a side effect of validating them, which is what let the rules stop
        // passing a mutable Counts around.
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
                cfg: &self.cfg,
                cwd: &self.cwd,
                name,
                description,
            };
            let mut sink = FindingSink {
                findings: &mut findings,
                tally: &mut tally,
                strict: self.cfg.strict,
                rule: "",
            };
            for rule in rules::all() {
                if !rule.applies_to(t.kind) || self.disabled.contains(rule.id()) {
                    continue;
                }
                sink.rule = rule.id();
                rule.check(&ctx, &mut sink);
            }
        }

        FileResult {
            path: t.path.clone(),
            kind: t.kind,
            score: score(&tokens, &self.cfg, t.kind, &tally),
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
    use crate::config::{
        DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
        DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS,
    };
    use crate::lint::ids::*;
    use crate::lint::test_utils::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn skill_frontmatter_rules() {
        let long_name = "a".repeat(MAX_NAME_CHARS + 1);
        let long_desc = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
        let cases: Vec<(&str, &str, String, Vec<&str>)> = vec![
            ("valid skill has no findings", "valid-skill", VALID_SKILL.to_string(), vec![]),
            (
                "missing front matter",
                "no-frontmatter",
                "# A skill with no front matter\n\nBody.\n".to_string(),
                vec![RULE_FRONTMATTER_MISSING],
            ),
            (
                "front matter not first",
                "late-frontmatter",
                "\n---\nname: late-frontmatter\ndescription: Late.\n---\n".to_string(),
                vec![RULE_FRONTMATTER_NOT_FIRST],
            ),
            (
                "unterminated front matter",
                "unterminated",
                "---\nname: unterminated\ndescription: No closing fence.\n".to_string(),
                vec![RULE_FRONTMATTER_UNTERMINATED],
            ),
            (
                "invalid yaml",
                "bad-yaml",
                "---\nname: bad-yaml\n  description: wrong indent\n---\nBody.\n".to_string(),
                vec![RULE_FRONTMATTER_INVALID],
            ),
            (
                "front matter is a sequence",
                "sequence",
                "---\n- name: sequence\n---\nBody.\n".to_string(),
                vec![RULE_FRONTMATTER_INVALID],
            ),
            (
                "name missing",
                "nameless",
                "---\ndescription: A skill with no name.\n---\nBody.\n".to_string(),
                vec![RULE_NAME_REQUIRED],
            ),
            (
                "name empty",
                "empty-name",
                "---\nname: \"\"\ndescription: A skill with an empty name.\n---\nBody.\n".to_string(),
                vec![RULE_NAME_REQUIRED],
            ),
            (
                "name not a scalar",
                "listy-name",
                "---\nname:\n  - listy-name\ndescription: A skill whose name is a list.\n---\nBody.\n"
                    .to_string(),
                vec![RULE_NAME_REQUIRED],
            ),
            (
                "name has bad characters",
                "Bad_Name",
                "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n".to_string(),
                vec![RULE_NAME_FORMAT],
            ),
            (
                "name has double hyphen",
                "double--hyphen",
                "---\nname: double--hyphen\ndescription: A skill with a doubled hyphen.\n---\nBody.\n"
                    .to_string(),
                vec![RULE_NAME_FORMAT],
            ),
            (
                "name too long",
                "long",
                format!(
                    "---\nname: {long_name}\ndescription: A skill with a very long name.\n---\nBody.\n"
                ),
                vec![RULE_NAME_LENGTH, RULE_NAME_DIR_MISMATCH],
            ),
            (
                "name does not match directory",
                "some-directory",
                "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n"
                    .to_string(),
                vec![RULE_NAME_DIR_MISMATCH],
            ),
            (
                "description missing",
                "no-description",
                "---\nname: no-description\n---\nBody.\n".to_string(),
                vec![RULE_DESCRIPTION_REQUIRED],
            ),
            (
                "description empty",
                "blank-description",
                "---\nname: blank-description\ndescription: \"   \"\n---\nBody.\n".to_string(),
                vec![RULE_DESCRIPTION_REQUIRED],
            ),
            (
                "description too long",
                "verbose",
                format!("---\nname: verbose\ndescription: {long_desc}\n---\nBody.\n"),
                vec![RULE_DESCRIPTION_LENGTH],
            ),
            (
                "unknown key",
                "extra-keys",
                "---\nname: extra-keys\ndescription: A skill with a stray key.\nauthor: someone\n---\nBody.\n"
                    .to_string(),
                vec![RULE_FRONTMATTER_UNKNOWN_KEY],
            ),
            (
                "known optional keys are accepted",
                "optional-keys",
                "---\nname: optional-keys\ndescription: A skill using every optional key.\nlicense: MIT\nallowed-tools:\n  - Read\n  - Bash\nmetadata:\n  owner: infra\n---\nBody.\n".to_string(),
                vec![],
            ),
            (
                "allowed-tools wrong type",
                "bad-tools",
                "---\nname: bad-tools\ndescription: A skill with a malformed tool list.\nallowed-tools:\n  read: true\n---\nBody.\n".to_string(),
                vec![RULE_ALLOWED_TOOLS_TYPE],
            ),
            (
                "metadata wrong type",
                "bad-metadata",
                "---\nname: bad-metadata\ndescription: A skill with scalar metadata.\nmetadata: not-a-mapping\n---\nBody.\n".to_string(),
                vec![RULE_METADATA_TYPE],
            ),
            (
                "several problems are all reported",
                "multi",
                "---\nname: Multi_Skill\nextra: true\n---\nBody.\n".to_string(),
                vec![
                    RULE_FRONTMATTER_UNKNOWN_KEY,
                    RULE_NAME_FORMAT,
                    RULE_NAME_DIR_MISMATCH,
                    RULE_DESCRIPTION_REQUIRED,
                ],
            ),
        ];

        for (name, skill_dir_name, src, want_rules) in cases {
            let (_base, target) = write_skill(skill_dir_name, &src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), want_rules, "{name}");
        }
    }

    #[test]
    fn token_budgets() {
        let (_base, target) = write_skill("budgeted", VALID_SKILL);

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 1;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_CONTENT]
        );

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 0;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_NAME_DIR_MISMATCH]);

        let mut cfg = generous_config();
        cfg.max_skill_name_tokens = 1;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_NAME]
        );

        let mut cfg = generous_config();
        cfg.max_skill_description_tokens = 2;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_DESCRIPTION]
        );

        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert!(res.tokens.content > 0 && res.tokens.name > 0 && res.tokens.description > 0);
    }

    #[test]
    fn per_kind_content_budget() {
        let base = tempfile::tempdir().unwrap();
        let body = "some filler prose to spend tokens on. ".repeat(20);
        let agents = agents_target(base.path(), &body);
        let (_skill_base, skill) = write_skill(
            "kinded",
            &format!("---\nname: kinded\ndescription: A skill with a body.\n---\n{body}"),
        );

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 10000;
        cfg.max_agents_tokens = 1;
        let linter = Linter::new(cfg.clone());

        let agents_res = linter.file(&agents).unwrap();
        assert_eq!(agents_res.errors(), 1);

        let skill_res = linter.file(&skill).unwrap();
        assert!(
            !skill_res
                .findings
                .iter()
                .any(|f| f.rule == RULE_TOKENS_CONTENT)
        );

        cfg.max_agents_tokens = 0;
        cfg.max_skill_tokens = 1;
        let skill_res = Linter::new(cfg).file(&skill).unwrap();
        assert!(
            skill_res
                .findings
                .iter()
                .any(|f| f.rule == RULE_TOKENS_CONTENT)
        );
    }

    #[test]
    fn agents_frontmatter_is_not_validated() {
        let base = tempfile::tempdir().unwrap();
        let src =
            "---\ntitle: Project instructions\nowner: infra\n---\n\n# Instructions\n\nBody.\n";
        let target = agents_target(base.path(), src);

        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert!(res.findings.is_empty());
        assert!(res.tokens.content > 0);
    }

    #[test]
    fn agents_unterminated_frontmatter_is_reported() {
        let base = tempfile::tempdir().unwrap();
        let src = "---\ntitle: Project instructions\n\n# Instructions never closed\n";
        let target = agents_target(base.path(), src);

        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FRONTMATTER_UNTERMINATED]);
    }

    #[test]
    fn agents_frontmatter_excluded_from_content() {
        let base = tempfile::tempdir().unwrap();
        let fm_src =
            "---\ntitle: A fairly wordy front matter block that costs real tokens\n---\nBody.\n";
        let with_fm = agents_target(base.path(), fm_src);
        let base2 = tempfile::tempdir().unwrap();
        let bare = agents_target(base2.path(), "Body.\n");

        let linter = Linter::new(generous_config());
        let with_res = linter.file(&with_fm).unwrap();
        let bare_res = linter.file(&bare).unwrap();
        assert_eq!(with_res.tokens.content, bare_res.tokens.content);
    }

    #[test]
    fn with_counter_substitutes_the_token_counter() {
        let (_base, target) = write_skill("counted", VALID_SKILL);

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 10;
        let res = Linter::new(cfg.clone())
            .with_counter(Box::new(Fixed(11)))
            .file(&target)
            .unwrap();
        assert_eq!(res.tokens.content, 11);
        assert!(res.findings.iter().any(|f| f.rule == RULE_TOKENS_CONTENT));

        let res = Linter::new(cfg)
            .with_counter(Box::new(Fixed(9)))
            .file(&target)
            .unwrap();
        assert_eq!(res.tokens.content, 9);
        assert!(!res.findings.iter().any(|f| f.rule == RULE_TOKENS_CONTENT));
    }

    #[test]
    fn name_dir_mismatch_uses_injected_cwd() {
        // name.dir-mismatch deliberately ignores a skill sitting directly in
        // the working directory, since a loose SKILL.md is named for its
        // project rather than its folder. Injecting the cwd is what makes that
        // branch testable: reading it from the process would mean chdir-ing
        // while the rest of the suite runs concurrently.
        let (base, target) = write_skill(
            "some-directory",
            "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n",
        );
        let holding_dir = Path::new(&target.path).parent().unwrap().to_path_buf();

        let res = Linter::new(generous_config())
            .with_cwd(holding_dir)
            .file(&target)
            .unwrap();
        assert!(res.findings.is_empty(), "{:?}", rule_ids(&res.findings));

        let res = Linter::new(generous_config())
            .with_cwd(base.path().to_path_buf())
            .file(&target)
            .unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_NAME_DIR_MISMATCH]);
    }

    #[test]
    fn default_config_enforces_budgets() {
        let cfg = Config::default();
        assert_eq!(cfg.max_agents_tokens, DEFAULT_MAX_AGENTS_TOKENS);
        assert_eq!(cfg.max_skill_tokens, DEFAULT_MAX_SKILL_TOKENS);
        assert_eq!(cfg.max_skill_name_tokens, DEFAULT_MAX_SKILL_NAME_TOKENS);
        assert_eq!(
            cfg.max_skill_description_tokens,
            DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS
        );

        // The point of the hand-written Default: reaching for it must not
        // hand back a linter that checks nothing.
        let base = tempfile::tempdir().unwrap();
        let body = "some filler prose to spend tokens on. ".repeat(500);
        let target = agents_target(base.path(), &body);
        let res = Linter::new(Config::default()).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_TOKENS_CONTENT]);
    }

    #[test]
    fn strict_promotes_warnings() {
        let (_base, target) = write_skill(
            "mismatched-dir",
            "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n",
        );

        let cfg = generous_config();
        let res = Linter::new(cfg.clone()).file(&target).unwrap();
        assert_eq!(res.errors(), 0);
        assert_eq!(res.warnings(), 1);

        let mut cfg = cfg;
        cfg.strict = true;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(res.errors(), 1);
        assert_eq!(res.warnings(), 0);
    }

    #[test]
    fn disable_skips_rules() {
        let (_base, target) = write_skill(
            "mismatched-dir",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
        );

        let mut cfg = generous_config();
        cfg.disabled = vec![
            RULE_NAME_FORMAT.to_string(),
            RULE_NAME_DIR_MISMATCH.to_string(),
        ];
        let res = Linter::new(cfg).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }

    #[test]
    fn findings_carry_file_and_line() {
        let (_base, target) = write_skill(
            "lines",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
        );

        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(res.path, target.path);
        for f in &res.findings {
            if f.rule == RULE_NAME_FORMAT {
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
        assert!(Linter::new(generous_config()).file(&target).is_err());
    }

    #[test]
    fn file_reference_rule() {
        {
            let dir = tempfile::tempdir().unwrap();
            write_file(dir.path(), "notes.md", "notes");
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "See [docs](https://example.com/x), [mail](mailto:a@example.com) and [section](#heading).\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "Example syntax:\n\n```md\n[example](./missing.md)\n```\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let (_base, target) = write_skill(
                "with-link",
                "---\nname: with-link\ndescription: A skill that references a missing helper script.\n---\nSee [helper](./helper.sh).\n",
            );
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nIntro line.\n\nSee [notes](./notes.md).\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(res.findings.len(), 1);
            assert_eq!(res.findings[0].line, 5);
        }
    }

    #[test]
    fn file_references_outside_the_linted_tree_are_skipped() {
        // Escapes are skipped whether or not the target exists. lintmatter runs
        // in CI over checkouts it does not trust, and a finding that
        // distinguished "exists" from "does not exist" above the root would
        // make any file's markdown a probe for what is on the host.
        let outer = tempfile::tempdir().unwrap();
        write_file(outer.path(), "outside.md", "a real file above the root");
        let root = outer.path().join("root");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let escaping: &[(&str, &str)] = &[
            (
                "link to an existing file above the root",
                "See [x](../../outside.md).\n",
            ),
            (
                "link to a missing file above the root",
                "See [x](../../nowhere.md).\n",
            ),
            ("code span escaping the root", "Read `../../outside.md`.\n"),
        ];
        for (name, src) in escaping {
            let target = agents_target_under(&root, &sub, src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(
                res.findings.is_empty(),
                "{name}: {:?}",
                rule_ids(&res.findings)
            );
        }

        // Control: a missing reference that stays inside the tree is still
        // reported, so the cases above are not passing vacuously.
        let target = agents_target_under(&root, &sub, "See [x](./nowhere.md).\n");
        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
    }

    #[test]
    fn file_reference_rule_nested_fences() {
        // A fence closes only on the same marker, at least as long, with no
        // info string. Each case pairs a source file with the rules it should
        // produce, so both directions of the old toggle bug are covered: the
        // block body must stay unscanned, and the tracker must not latch open
        // and swallow the prose that follows.
        let cases: &[(&str, &str, Vec<&str>)] = &[
            (
                "inner ``` does not close an outer ````",
                "````md\n```\n[x](./missing.md)\n```\nstill inside: [y](./gone.md)\n````\n",
                vec![],
            ),
            (
                "~~~ does not close a ``` block",
                "```\n~~~\n[x](./missing.md)\n~~~\n```\n",
                vec![],
            ),
            (
                "a fence carrying an info string does not close",
                "```\n```rust\n[x](./missing.md)\n```\n",
                vec![],
            ),
            (
                "prose after a closed block is still scanned",
                "````\n```\n````\n\nSee [y](./gone.md).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
            (
                "a longer run closes a shorter block",
                "```\n[x](./missing.md)\n````\n\nSee [y](./gone.md).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
            (
                "an indented closing fence still closes",
                "```\n[x](./missing.md)\n  ```\n\nSee [y](./gone.md).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
        ];

        for (name, src, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), *want, "{name}");
        }
    }

    #[test]
    fn file_reference_rule_code_spans() {
        {
            // Path-shaped inline code span pointing at a missing file inside
            // the tree.
            let dir = tempfile::tempdir().unwrap();
            let sub = dir.path().join("sub");
            fs::create_dir_all(&sub).unwrap();
            let src = "Read instructions from `../planner_instructions.md`.\n";
            let target = agents_target_under(dir.path(), &sub, src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            // Same reference, but the file exists. A `../` hop that lands back
            // inside the linted tree must still be checked -- this is the
            // guard that skipping escapes did not swallow ordinary references.
            let dir = tempfile::tempdir().unwrap();
            let sub = dir.path().join("sub");
            fs::create_dir_all(&sub).unwrap();
            write_file(dir.path(), "planner_instructions.md", "notes");
            let src = "Read instructions from `../planner_instructions.md`.\n";
            let target = agents_target_under(dir.path(), &sub, src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            // Code spans that are not path-shaped are left alone.
            let dir = tempfile::tempdir().unwrap();
            let src = "Run `cargo test`, check `--strict`, or see `notes` and `./bin/lint`.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            // A link and a code span pointing at the same missing target on
            // one line report once, not twice.
            let dir = tempfile::tempdir().unwrap();
            let src = "See [it](./missing.md) or `./missing.md`.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            // Code spans inside fenced code blocks are still ignored.
            let dir = tempfile::tempdir().unwrap();
            let src = "Example:\n\n```md\nSee `../missing.md`.\n```\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
    }

    #[test]
    fn file_reference_rule_links_inside_code_spans() {
        // Link syntax shown inside backticks is literal text, not a link, so
        // its target is not checked. A code span used as a link *label* is the
        // other direction of the same masking: that link is real and must
        // still be checked.
        let cases: &[(&str, &str, Vec<&str>)] = &[
            (
                "link syntax inside a code span is not a reference",
                "Write it as `[title](./missing.md)`.\n",
                vec![],
            ),
            (
                "an image inside a code span is not a reference",
                "Write it as `![alt](./missing.png)`.\n",
                vec![],
            ),
            (
                "a double-backtick span hides link syntax too",
                "Use ``[title](./missing.md)`` for that.\n",
                vec![],
            ),
            (
                "a code span in the label does not hide the link",
                "See [`notes`](./notes.md).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
            (
                // The masked target is dropped rather than reported as a file
                // named `***`; the span is still read as a path on its own.
                "a masked span in the target position is not reported as `***`",
                "See [notes](`./notes.md`).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
            (
                "masking one link leaves a real one on the same line",
                "See `[x](./missing.md)` and [y](./gone.md).\n",
                vec![RULE_FILE_REFERENCE_MISSING],
            ),
        ];

        for (name, src, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), *want, "{name}");
        }

        // The label case again with the file present: the link is checked, and
        // it resolves, so the case above is not passing on a parse failure.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "notes.md", "notes");
        let target = agents_target(dir.path(), "See [`notes`](./notes.md).\n");
        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
}
