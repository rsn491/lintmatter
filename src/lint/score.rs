use crate::config::Config;
use crate::discover::Kind;

use super::finding::Counts;
use super::ids::*;
use super::sink::{Part, Tally};

/// Rates one file from 0 to 100: the mean of its frontmatter, token-budget and
/// file-reference components.
///
/// A component with nothing to judge is left out of the mean rather than
/// counted as perfect, so an inapplicable check cannot inflate a score. That
/// covers an AGENTS.md, whose front matter lintmatter deliberately does not
/// validate; a file that names no checkable reference; and a budget switched
/// off. A file with no applicable component at all rates 100: there was
/// nothing to hold against it.
pub(super) fn score(tokens: &Counts, cfg: &Config, kind: Kind, tally: &Tally) -> u8 {
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
    use crate::lint::Linter;
    use crate::lint::ids::*;
    use crate::lint::test_utils::*;

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
            let res = Linter::new(generous_config()).file(&target).unwrap();
            assert_eq!(res.score, want, "{name}");
        }
    }

    /// A disabled rule was never applied, so it leaves the fraction entirely;
    /// --strict only moves severities, which the score does not read.
    #[test]
    fn score_follows_disable_but_not_strict() {
        let src = "---\nname: Bad_Name\ndescription: Use this skill for a fixture.\n---\nBody.\n";
        let (_base, target) = write_skill("Bad_Name", src);

        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(res.score, 87);

        let mut cfg = generous_config();
        cfg.strict = true;
        let res = Linter::new(cfg).file(&target).unwrap();
        assert_eq!(res.score, 87);

        let mut cfg = generous_config();
        cfg.disabled = vec![RULE_NAME_FORMAT.to_string()];
        let res = Linter::new(cfg).file(&target).unwrap();
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
            let linter = Linter::new(cfg.clone()).with_counter(Box::new(Fixed(count)));
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
        let res = Linter::new(cfg.clone())
            .with_counter(Box::new(Fixed(2)))
            .file(&target)
            .unwrap();
        // Front matter is perfect, the one budget that ran scores zero.
        assert_eq!(res.score, 50);

        // Budget the name too and it passes, pulling the token part back to 50.
        cfg.max_skill_name_tokens = 2;
        let res = Linter::new(cfg)
            .with_counter(Box::new(Fixed(2)))
            .file(&target)
            .unwrap();
        assert_eq!(res.score, 75);
    }

    #[test]
    fn file_reference_score_is_all_or_nothing() {
        let base = tempfile::tempdir().unwrap();
        write_file(base.path(), "there.md", "Present.\n");
        let target = agents_target(base.path(), "See [there](./there.md).\n");
        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(res.score, 100);

        let base = tempfile::tempdir().unwrap();
        write_file(base.path(), "there.md", "Present.\n");
        let target = agents_target(
            base.path(),
            "See [there](./there.md) and [gone](./gone.md).\n",
        );
        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(res.score, 0);
    }

    /// An AGENTS.md has no front matter to rate, and with every budget off and
    /// nothing to resolve there is nothing left to hold against it.
    #[test]
    fn parts_with_nothing_to_judge_drop_out() {
        let base = tempfile::tempdir().unwrap();
        let target = agents_target(base.path(), "Body with no references.\n");
        let res = Linter::new(generous_config()).file(&target).unwrap();
        assert_eq!(res.score, 100);

        // Give it a broken reference and a body twice its budget: the two parts
        // that now apply average, with front matter still left out.
        let target = agents_target(base.path(), "See [gone](./gone.md).\n");
        let mut cfg = generous_config();
        cfg.max_agents_tokens = 100;
        let res = Linter::new(cfg)
            .with_counter(Box::new(Fixed(200)))
            .file(&target)
            .unwrap();
        assert_eq!(res.score, 0);
    }
}
