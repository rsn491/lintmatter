//! Applies lintmatter's rules to agent instruction files.

pub mod rule;
pub mod rules;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::config::Config;
use crate::discover::{Kind, Target};
use crate::parse::{self, Document, ErrKind, Frontmatter};
use crate::tokens::{self, Counter};
use crate::utils::clean_path;

// Rule ids. Listed here so callers -- and `--disable` -- can name a rule
// without knowing which module implements it; each rule module imports the one
// it answers to. Report order is NOT set here: that lives solely in the
// registry, `rules::all`.
pub const RULE_FRONTMATTER_MISSING: &str = "frontmatter.missing";
pub const RULE_FRONTMATTER_NOT_FIRST: &str = "frontmatter.not-first";
pub const RULE_FRONTMATTER_UNTERMINATED: &str = "frontmatter.unterminated";
pub const RULE_FRONTMATTER_INVALID: &str = "frontmatter.invalid";
pub const RULE_FRONTMATTER_UNKNOWN_KEY: &str = "frontmatter.unknown-key";
pub const RULE_NAME_REQUIRED: &str = "name.required";
pub const RULE_NAME_FORMAT: &str = "name.format";
pub const RULE_NAME_LENGTH: &str = "name.length";
pub const RULE_NAME_DIR_MISMATCH: &str = "name.dir-mismatch";
pub const RULE_DESCRIPTION_REQUIRED: &str = "description.required";
pub const RULE_DESCRIPTION_LENGTH: &str = "description.length";
pub const RULE_ALLOWED_TOOLS_TYPE: &str = "allowed-tools.type";
pub const RULE_METADATA_TYPE: &str = "metadata.type";
pub const RULE_TOKENS_CONTENT: &str = "tokens.content";
pub const RULE_TOKENS_NAME: &str = "tokens.name";
pub const RULE_TOKENS_DESCRIPTION: &str = "tokens.description";
pub const RULE_FILE_REFERENCE_MISSING: &str = "file-reference.missing";

/// Limits from the Anthropic skill spec, in characters.
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;

/// Every rule id lintmatter can emit, in report order. Derived from the registry,
/// so it cannot drift from the rules that actually run.
pub static RULES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| rules::all().iter().map(|r| r.id()).collect());

/// The part of a file's score a rule's outcome feeds. Every id in [`RULES`]
/// maps to one, enforced by a test, so a new rule cannot be added without
/// deciding what it rates. Named `Part` rather than the more natural
/// `Component` because `std::path::Component` already holds that name here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Frontmatter,
    Tokens,
    FileReferences,
}

/// Spelled out per rule rather than falling back to a catch-all arm: the
/// fallback is what would let a new rule slip into the wrong component.
fn part(rule: &str) -> Option<Part> {
    match rule {
        RULE_FRONTMATTER_MISSING
        | RULE_FRONTMATTER_NOT_FIRST
        | RULE_FRONTMATTER_UNTERMINATED
        | RULE_FRONTMATTER_INVALID
        | RULE_FRONTMATTER_UNKNOWN_KEY
        | RULE_NAME_REQUIRED
        | RULE_NAME_FORMAT
        | RULE_NAME_LENGTH
        | RULE_NAME_DIR_MISMATCH
        | RULE_DESCRIPTION_REQUIRED
        | RULE_DESCRIPTION_LENGTH
        | RULE_ALLOWED_TOOLS_TYPE
        | RULE_METADATA_TYPE => Some(Part::Frontmatter),
        RULE_TOKENS_CONTENT | RULE_TOKENS_NAME | RULE_TOKENS_DESCRIPTION => Some(Part::Tokens),
        RULE_FILE_REFERENCE_MISSING => Some(Part::FileReferences),
        _ => None,
    }
}

/// Marks how a finding affects the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// A single rule violation.
///
/// The file is not stored here: it is always the enclosing [`FileResult`]'s
/// path, so keeping a copy per finding meant an allocation each. The JSON
/// report still carries `file` on every finding -- the reporter fills it in
/// from the result it is already walking.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Finding {
    #[serde(skip_serializing_if = "is_zero")]
    pub line: usize,
    /// Always one of the registry's ids, so it borrows rather than allocates.
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The measured token cost of a file's parts.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Counts {
    pub content: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub name: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub description: usize,
}

/// Everything lintmatter learned about one file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileResult {
    pub path: String,
    pub kind: Kind,
    pub tokens: Counts,
    /// How well the file rates, 0 to 100. See [`score`].
    pub score: u8,
    pub findings: Vec<Finding>,
}

impl FileResult {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == s).count()
    }
}

/// Which rules ran on a file and which of them fired. Tracked per rule id, not
/// per finding, so a rule that reports repeatedly -- an unknown key on every
/// line, a broken link on every line -- still counts once toward the score.
#[derive(Debug, Default)]
struct Tally {
    applied: HashSet<&'static str>,
    failed: HashSet<&'static str>,
}

impl Tally {
    /// Returns how many of a component's rules ran, and how many of those
    /// fired.
    fn counts(&self, p: Part) -> (usize, usize) {
        let of = |set: &HashSet<&'static str>| set.iter().filter(|r| part(r) == Some(p)).count();
        (of(&self.applied), of(&self.failed))
    }
}

/// Collects findings for the rule currently running, applying `--strict`.
///
/// Named for what it does rather than "Reporter", which the `report` module
/// already means. The rule id comes from the sink rather than each call site,
/// so a rule cannot file a finding under someone else's id.
pub struct FindingSink<'a> {
    findings: &'a mut Vec<Finding>,
    tally: &'a mut Tally,
    strict: bool,
    rule: &'static str,
}

impl FindingSink<'_> {
    /// Records a violation that should fail the build.
    pub fn error(&mut self, line: usize, msg: String) {
        self.add(Severity::Error, line, msg);
    }

    /// Records a stylistic mismatch. `--strict` promotes it to an error.
    pub fn warn(&mut self, line: usize, msg: String) {
        self.add(Severity::Warning, line, msg);
    }

    /// Records that the current rule had something to judge on this file,
    /// whether or not it goes on to fire. A rule that never calls this --
    /// because its precondition never held, the way `allowed-tools.type`
    /// stays quiet when the key is absent -- leaves the score's numerator and
    /// denominator alike, the same way a disabled rule already does by never
    /// running its `check` at all.
    pub fn applies(&mut self) {
        self.tally.applied.insert(self.rule);
    }

    fn add(&mut self, mut sev: Severity, line: usize, msg: String) {
        // A rule that fired must be in the denominator too, whether or not the
        // check site remembered to call `applies` first.
        self.tally.applied.insert(self.rule);
        self.tally.failed.insert(self.rule);
        if sev == Severity::Warning && self.strict {
            sev = Severity::Error;
        }
        self.findings.push(Finding {
            line,
            rule: self.rule,
            severity: sev,
            message: msg,
        });
    }
}

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

impl FileContext<'_> {
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
    /// Returns a Linter anchored to the process's working directory. `None`
    /// for `counter` falls back to the heuristic estimator.
    pub fn new(cfg: Config, counter: Option<Box<dyn Counter>>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self::with_cwd(cfg, counter, cwd)
    }

    /// Returns a Linter that treats `cwd` as the working directory, so the one
    /// rule that cares can be tested without mutating process-global state
    /// while other tests run alongside it.
    pub fn with_cwd(cfg: Config, counter: Option<Box<dyn Counter>>, cwd: PathBuf) -> Self {
        let counter = counter.unwrap_or_else(|| Box::new(tokens::Estimator::new()));
        let disabled = cfg.disabled.iter().cloned().collect();
        Linter {
            cfg,
            counter,
            disabled,
            cwd,
        }
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

/// Rates one file from 0 to 100: the mean of its frontmatter, token-budget and
/// file-reference components.
///
/// A component with nothing to judge is left out of the mean rather than
/// counted as perfect, so an inapplicable check cannot inflate a score. That
/// covers an AGENTS.md, whose front matter lintmatter deliberately does not
/// validate; a file that names no checkable reference; and a budget switched
/// off. A file with no applicable component at all rates 100: there was
/// nothing to hold against it.
fn score(tokens: &Counts, cfg: &Config, kind: Kind, tally: &Tally) -> u8 {
    let mut parts: Vec<f64> = Vec::new();

    let (applied, failed) = tally.counts(Part::Frontmatter);
    if applied > 0 {
        parts.push(100.0 * (applied - failed) as f64 / applied as f64);
    }

    // One sub-score per budget that was actually checked, averaged, so a
    // skill's name and description weigh against its body rather than beside
    // the other two components.
    let budgets: Vec<f64> = [
        (RULE_TOKENS_CONTENT, tokens.content, cfg.content_limit(kind)),
        (RULE_TOKENS_NAME, tokens.name, cfg.max_skill_name_tokens),
        (
            RULE_TOKENS_DESCRIPTION,
            tokens.description,
            cfg.max_skill_description_tokens,
        ),
    ]
    .into_iter()
    .filter(|(rule, _, _)| tally.applied.contains(rule))
    .map(|(_, count, limit)| budget_score(count, limit))
    .collect();
    if !budgets.is_empty() {
        parts.push(budgets.iter().sum::<f64>() / budgets.len() as f64);
    }

    if tally.applied.contains(RULE_FILE_REFERENCE_MISSING) {
        let broken = tally.failed.contains(RULE_FILE_REFERENCE_MISSING);
        parts.push(if broken { 0.0 } else { 100.0 });
    }

    if parts.is_empty() {
        return 100;
    }
    let mean = parts.iter().sum::<f64>() / parts.len() as f64;
    // Floored, so a score only reaches a band it has fully earned: 87.5 rates
    // 87 rather than being rounded up into an 88 the file did not achieve.
    mean.clamp(0.0, 100.0).floor() as u8
}

/// Scores one token budget: full marks at or under the limit, then falling
/// linearly to zero at twice the limit. Graded rather than pass/fail so a file
/// a little over its budget does not rate the same as one five times over.
fn budget_score(count: usize, limit: i64) -> f64 {
    if limit <= 0 || count as i64 <= limit {
        return 100.0;
    }
    let over = (count as i64 - limit) as f64 / limit as f64;
    100.0 * (1.0 - over.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
        DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS,
    };
    use std::fs;

    fn write_skill(skill_name: &str, src: &str) -> (tempfile::TempDir, Target) {
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

    fn write_file(dir: &Path, name: &str, src: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, src).unwrap();
        path.to_string_lossy().to_string()
    }

    fn agents_target(dir: &Path, src: &str) -> Target {
        agents_target_under(dir, dir, src)
    }

    /// An AGENTS.md written into `dir`, but discovered under walk root `root`.
    /// Separate from `agents_target` so a fixture can sit below its root, which
    /// is what makes a `../` reference back into the tree distinguishable from
    /// one that escapes it.
    fn agents_target_under(root: &Path, dir: &Path, src: &str) -> Target {
        Target {
            path: write_file(dir, "AGENTS.md", src),
            kind: Kind::Agents,
            root: root.to_path_buf(),
        }
    }

    fn rule_ids(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.rule).collect()
    }

    /// Every budget off, so rule tests see only the rule under test. Spelled
    /// out rather than derived from Config::default(), which now carries the
    /// real budgets -- keep it explicit.
    fn generous_config() -> Config {
        Config {
            max_agents_tokens: 0,
            max_skill_tokens: 0,
            max_skill_name_tokens: 0,
            max_skill_description_tokens: 0,
            disabled: vec![],
            strict: false,
        }
    }

    const VALID_SKILL: &str = "---\nname: valid-skill\ndescription: Use this skill when you need a fixture that passes every rule.\n---\n\n# Valid skill\n\nBody content.\n";

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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), want_rules, "{name}");
        }
    }

    #[test]
    fn token_budgets() {
        let (_base, target) = write_skill("budgeted", VALID_SKILL);

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 1;
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_CONTENT]
        );

        let mut cfg = generous_config();
        cfg.max_skill_tokens = 0;
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_NAME_DIR_MISMATCH]);

        let mut cfg = generous_config();
        cfg.max_skill_name_tokens = 1;
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_NAME]
        );

        let mut cfg = generous_config();
        cfg.max_skill_description_tokens = 2;
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_DESCRIPTION]
        );

        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.tokens.content > 0 && res.tokens.name > 0 && res.tokens.description > 0);
    }

    /// A counter reporting a fixed number, so a score test can sit exactly on
    /// a budget boundary instead of depending on the estimator's heuristics.
    struct Fixed(usize);

    impl Counter for Fixed {
        fn count(&self, _: &str) -> usize {
            self.0
        }
    }

    /// Guards the score against a rule being added and rated by accident: a
    /// new id has to be given a part before this passes.
    #[test]
    fn every_rule_belongs_to_a_score_part() {
        for rule in RULES.iter() {
            assert!(part(rule).is_some(), "{rule} is not assigned a score part");
        }
    }

    #[test]
    fn frontmatter_score() {
        let valid_body = "description: Use this skill when you need a fixture that passes.";
        let cases: Vec<(&str, &str, String, u8)> = vec![
            (
                "clean skill passes every rule applied to it",
                "valid-skill",
                VALID_SKILL.to_string(),
                100,
            ),
            (
                "one of eight rules fails, and 87.5 floors to 87",
                "Bad_Name",
                format!("---\nname: Bad_Name\n{valid_body}\n---\nBody.\n"),
                87,
            ),
            (
                "a rule firing repeatedly still costs one rule",
                "valid-skill",
                format!(
                    "---\nname: valid-skill\n{valid_body}\nauthor: a\nversion: 1\nowner: b\n---\nBody.\n"
                ),
                87,
            ),
            (
                "missing front matter leaves one rule applied, and it failed",
                "no-frontmatter",
                "# No front matter\n\nBody.\n".to_string(),
                0,
            ),
            (
                "unparseable front matter scores the same as none",
                "bad-yaml",
                "---\nname: bad-yaml\n  description: wrong indent\n---\nBody.\n".to_string(),
                0,
            ),
        ];

        for (name, dir, src, want) in cases {
            let (_base, target) = write_skill(dir, &src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(res.score, want, "{name}");
        }
    }

    /// A disabled rule was never applied, so it leaves the fraction entirely;
    /// --strict only moves severities, which the score does not read.
    #[test]
    fn score_follows_disable_but_not_strict() {
        let src = "---\nname: Bad_Name\ndescription: Use this skill for a fixture.\n---\nBody.\n";
        let (_base, target) = write_skill("Bad_Name", src);

        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(res.score, 87);

        let mut cfg = generous_config();
        cfg.strict = true;
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(res.score, 87);

        let mut cfg = generous_config();
        cfg.disabled = vec![RULE_NAME_FORMAT.to_string()];
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert_eq!(res.score, 100);
    }

    /// Full marks up to the budget, then falling linearly to zero at twice it.
    #[test]
    fn token_score_grades_the_overage() {
        let base = tempfile::tempdir().unwrap();
        let target = agents_target(base.path(), "Body.\n");
        let mut cfg = generous_config();
        cfg.max_agents_tokens = 100;

        for (count, want) in [
            (50, 100),
            (100, 100),
            (125, 75),
            (150, 50),
            (200, 0),
            (900, 0),
        ] {
            let linter = Linter::new(cfg.clone(), Some(Box::new(Fixed(count))));
            let res = linter.file(&target).unwrap();
            assert_eq!(res.score, want, "{count} tokens against a 100 token budget");
        }
    }

    /// A skill's three budgets average into one component, so name and
    /// description weigh against the body rather than beside the other parts.
    #[test]
    fn token_score_averages_the_budgets_that_ran() {
        let (_base, target) = write_skill("valid-skill", VALID_SKILL);
        let mut cfg = generous_config();
        // Only the body is budgeted, and it sits at twice its limit.
        cfg.max_skill_tokens = 1;
        let res = Linter::new(cfg.clone(), Some(Box::new(Fixed(2))))
            .file(&target)
            .unwrap();
        // Front matter is perfect, the one budget that ran scores zero.
        assert_eq!(res.score, 50);

        // Budget the name too and it passes, pulling the token part back to 50.
        cfg.max_skill_name_tokens = 2;
        let res = Linter::new(cfg, Some(Box::new(Fixed(2))))
            .file(&target)
            .unwrap();
        assert_eq!(res.score, 75);
    }

    #[test]
    fn file_reference_score_is_all_or_nothing() {
        let base = tempfile::tempdir().unwrap();
        write_file(base.path(), "there.md", "Present.\n");
        let target = agents_target(base.path(), "See [there](./there.md).\n");
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(res.score, 100);

        let base = tempfile::tempdir().unwrap();
        write_file(base.path(), "there.md", "Present.\n");
        let target = agents_target(
            base.path(),
            "See [there](./there.md) and [gone](./gone.md).\n",
        );
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(res.score, 0);
    }

    /// An AGENTS.md has no front matter to rate, and with every budget off and
    /// nothing to resolve there is nothing left to hold against it.
    #[test]
    fn parts_with_nothing_to_judge_drop_out() {
        let base = tempfile::tempdir().unwrap();
        let target = agents_target(base.path(), "Body with no references.\n");
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(res.score, 100);

        // Give it a broken reference and a body twice its budget: the two parts
        // that now apply average, with front matter still left out.
        let target = agents_target(base.path(), "See [gone](./gone.md).\n");
        let mut cfg = generous_config();
        cfg.max_agents_tokens = 100;
        let res = Linter::new(cfg, Some(Box::new(Fixed(200))))
            .file(&target)
            .unwrap();
        assert_eq!(res.score, 0);
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
        let linter = Linter::new(cfg.clone(), None);

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
        let skill_res = Linter::new(cfg, None).file(&skill).unwrap();
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

        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
        assert!(res.tokens.content > 0);
    }

    #[test]
    fn agents_unterminated_frontmatter_is_reported() {
        let base = tempfile::tempdir().unwrap();
        let src = "---\ntitle: Project instructions\n\n# Instructions never closed\n";
        let target = agents_target(base.path(), src);

        let res = Linter::new(generous_config(), None).file(&target).unwrap();
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

        let linter = Linter::new(generous_config(), None);
        let with_res = linter.file(&with_fm).unwrap();
        let bare_res = linter.file(&bare).unwrap();
        assert_eq!(with_res.tokens.content, bare_res.tokens.content);
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

        let res = Linter::with_cwd(generous_config(), None, holding_dir)
            .file(&target)
            .unwrap();
        assert!(res.findings.is_empty(), "{:?}", rule_ids(&res.findings));

        let res = Linter::with_cwd(generous_config(), None, base.path().to_path_buf())
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
        let res = Linter::new(Config::default(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_TOKENS_CONTENT]);
    }

    #[test]
    fn strict_promotes_warnings() {
        let (_base, target) = write_skill(
            "mismatched-dir",
            "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n",
        );

        let cfg = generous_config();
        let res = Linter::new(cfg.clone(), None).file(&target).unwrap();
        assert_eq!(res.errors(), 0);
        assert_eq!(res.warnings(), 1);

        let mut cfg = cfg;
        cfg.strict = true;
        let res = Linter::new(cfg, None).file(&target).unwrap();
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
        let res = Linter::new(cfg, None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }

    #[test]
    fn findings_carry_file_and_line() {
        let (_base, target) = write_skill(
            "lines",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
        );

        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        // The file is carried once, by the result; findings anchor the line.
        // That the JSON still repeats `file` on each finding is asserted by
        // the CLI's json_output test.
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
        assert!(Linter::new(generous_config(), None).file(&target).is_err());
    }

    #[test]
    fn file_reference_rule() {
        {
            let dir = tempfile::tempdir().unwrap();
            write_file(dir.path(), "notes.md", "notes");
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "See [docs](https://example.com/x), [mail](mailto:a@example.com) and [section](#heading).\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "Example syntax:\n\n```md\n[example](./missing.md)\n```\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            let (_base, target) = write_skill(
                "with-link",
                "---\nname: with-link\ndescription: A skill that references a missing helper script.\n---\nSee [helper](./helper.sh).\n",
            );
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nIntro line.\n\nSee [notes](./notes.md).\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(
                res.findings.is_empty(),
                "{name}: {:?}",
                rule_ids(&res.findings)
            );
        }

        // Control: a missing reference that stays inside the tree is still
        // reported, so the cases above are not passing vacuously.
        let target = agents_target_under(&root, &sub, "See [x](./nowhere.md).\n");
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            // Code spans that are not path-shaped are left alone.
            let dir = tempfile::tempdir().unwrap();
            let src = "Run `cargo test`, check `--strict`, or see `notes` and `./bin/lint`.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert!(res.findings.is_empty());
        }
        {
            // A link and a code span pointing at the same missing target on
            // one line report once, not twice.
            let dir = tempfile::tempdir().unwrap();
            let src = "See [it](./missing.md) or `./missing.md`.\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
        }
        {
            // Code spans inside fenced code blocks are still ignored.
            let dir = tempfile::tempdir().unwrap();
            let src = "Example:\n\n```md\nSee `../missing.md`.\n```\n";
            let target = agents_target(dir.path(), src);
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            let res = Linter::new(generous_config(), None).file(&target).unwrap();
            assert_eq!(rule_ids(&res.findings), *want, "{name}");
        }

        // The label case again with the file present: the link is checked, and
        // it resolves, so the case above is not passing on a parse failure.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "notes.md", "notes");
        let target = agents_target(dir.path(), "See [`notes`](./notes.md).\n");
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
}
