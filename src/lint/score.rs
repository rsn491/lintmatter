//! Rates a file from 0 to 100 from how its rules fared.

use super::rule::Part;
use super::sink::Tally;

/// The parts a score averages, in the order they are summed.
const PARTS: [Part; 3] = [Part::Frontmatter, Part::Tokens, Part::FileReferences];

/// Rates one file from 0 to 100: the mean of its frontmatter, token-budget and
/// file-reference parts, each the mean of the rules in it that ran. A
/// pass/fail rule scores 100 or 0; a graded one, such as a token budget,
/// scores where it landed. So a skill's three budgets average into one part,
/// and its name and description weigh against its body rather than beside
/// the other parts.
///
/// A part with nothing to judge is left out of the mean rather than counted
/// as perfect, so an inapplicable check cannot inflate a score. That covers an
/// AGENTS.md, whose front matter lintmatter deliberately does not validate; a
/// file that names no checkable reference; and a budget switched off. A file
/// with no applicable part at all rates 100: there was nothing to hold
/// against it.
pub(super) fn score(tally: &Tally) -> u8 {
    let parts: Vec<f64> = PARTS
        .iter()
        .filter_map(|&part| {
            let scores: Vec<f64> = tally
                .outcomes
                .iter()
                .filter(|o| o.part == part)
                .map(|o| o.score())
                .collect();
            mean(&scores)
        })
        .collect();

    match mean(&parts) {
        // Floored, so a score only reaches a band it has fully earned: 87.5
        // rates 87 rather than being rounded up into an 88 the file did not
        // achieve.
        Some(m) => m.clamp(0.0, 100.0).floor() as u8,
        None => 100,
    }
}

fn mean(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        None
    } else {
        Some(xs.iter().sum::<f64>() / xs.len() as f64)
    }
}

/// Scores one token budget: full marks at or under the limit, then falling
/// linearly to zero at twice the limit. Graded rather than pass/fail so a file
/// a little over its budget does not rate the same as one five times over.
pub(crate) fn budget_score(count: usize, limit: i64) -> f64 {
    if limit <= 0 || count as i64 <= limit {
        return 100.0;
    }
    let over = (count as i64 - limit) as f64 / limit as f64;
    100.0 * (1.0 - over.min(1.0))
}

#[cfg(test)]
mod tests {
    use crate::lint::testing::*;

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
            let res = lint(generous(), &target);
            assert_eq!(res.score, want, "{name}");
        }
    }

    /// A disabled rule was never applied, so it leaves the fraction entirely.
    #[test]
    fn score_follows_disable() {
        let src = "---\nname: Bad_Name\ndescription: Use this skill for a fixture.\n---\nBody.\n";
        let (_base, target) = write_skill("Bad_Name", src);

        let res = lint(generous(), &target);
        assert_eq!(res.score, 87);

        let mut opts = generous();
        opts.disabled = vec!["name.format".to_string()];
        let res = linter(opts).file(&target).unwrap();
        assert_eq!(res.score, 100);
    }

    /// Full marks up to the budget, then falling linearly to zero at twice it.
    #[test]
    fn token_score_grades_the_overage() {
        let base = tempfile::tempdir().unwrap();
        let target = agents_target(base.path(), "Body.\n");
        let mut opts = generous();
        opts.budgets.agents = 100;

        for (count, want) in [
            (50, 100),
            (100, 100),
            (125, 75),
            (150, 50),
            (200, 0),
            (900, 0),
        ] {
            let checker = linter(opts.clone()).with_counter(Box::new(Fixed(count)));
            let res = checker.file(&target).unwrap();
            assert_eq!(res.score, want, "{count} tokens against a 100 token budget");
        }
    }

    /// A skill's three budgets average into one component, so name and
    /// description weigh against the body rather than beside the other parts.
    #[test]
    fn token_score_averages_the_budgets_that_ran() {
        let (_base, target) = write_skill("valid-skill", VALID_SKILL);
        let mut opts = generous();
        // Only the body is budgeted, and it sits at twice its limit.
        opts.budgets.skill = 1;
        let res = linter(opts.clone())
            .with_counter(Box::new(Fixed(2)))
            .file(&target)
            .unwrap();
        // Front matter is perfect, the one budget that ran scores zero.
        assert_eq!(res.score, 50);

        // Budget the name too and it passes, pulling the token part back to 50.
        opts.budgets.skill_name = 2;
        let res = linter(opts)
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
        let res = lint(generous(), &target);
        assert_eq!(res.score, 100);

        let base = tempfile::tempdir().unwrap();
        write_file(base.path(), "there.md", "Present.\n");
        let target = agents_target(
            base.path(),
            "See [there](./there.md) and [gone](./gone.md).\n",
        );
        let res = lint(generous(), &target);
        assert_eq!(res.score, 0);
    }

    /// An AGENTS.md has no front matter to rate, and with every budget off and
    /// nothing to resolve there is nothing left to hold against it.
    #[test]
    fn parts_with_nothing_to_judge_drop_out() {
        let base = tempfile::tempdir().unwrap();
        let target = agents_target(base.path(), "Body with no references.\n");
        let res = lint(generous(), &target);
        assert_eq!(res.score, 100);

        // Give it a broken reference and a body twice its budget: the two parts
        // that now apply average, with front matter still left out.
        let target = agents_target(base.path(), "See [gone](./gone.md).\n");
        let mut opts = generous();
        opts.budgets.agents = 100;
        let res = linter(opts)
            .with_counter(Box::new(Fixed(200)))
            .file(&target)
            .unwrap();
        assert_eq!(res.score, 0);
    }
}
