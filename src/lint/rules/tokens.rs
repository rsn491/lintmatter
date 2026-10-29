//! Token budget rules.
//!
//! The counts themselves are measured once by the linter before any rule runs;
//! these rules only compare them against the configured budgets. A budget of
//! zero disables its check.

use crate::lint::rule::{ALL_KINDS, Part, Rule, RuleMeta, SKILL_ONLY};
use crate::lint::rules::humanize;
use crate::lint::score::budget_score;
use crate::lint::{FileContext, FindingSink};

/// Grades a measured count against its budget, and reports it when over.
/// Graded rather than pass/fail, so a file a little over its budget does not
/// rate the same as one five times over.
fn over_budget(sink: &mut FindingSink<'_>, line: usize, what: &str, got: usize, limit: i64) {
    sink.grade(budget_score(got, limit));
    if got as i64 <= limit {
        return;
    }
    sink.error(
        line,
        format!(
            "{what} is {} tokens, over the {} token limit",
            humanize(got as i64),
            humanize(limit)
        ),
    );
}

/// Applies to both kinds: an over-long AGENTS.md costs the model just as much
/// as an over-long skill.
pub struct Content;

impl Rule for Content {
    const META: RuleMeta = RuleMeta {
        id: "tokens.content",
        part: Part::Tokens,
        kinds: ALL_KINDS,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let limit = ctx.budgets.content_limit(ctx.target.kind);
        if limit <= 0 {
            return;
        }
        over_budget(sink, 0, "content", ctx.tokens.content, limit);
    }
}

pub struct Name;

impl Rule for Name {
    const META: RuleMeta = RuleMeta {
        id: "tokens.name",
        part: Part::Tokens,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        if ctx.budgets.skill_name <= 0 {
            return;
        }
        over_budget(
            sink,
            fm.line("name"),
            "name",
            ctx.tokens.name,
            ctx.budgets.skill_name,
        );
    }
}

pub struct Description;

impl Rule for Description {
    const META: RuleMeta = RuleMeta {
        id: "tokens.description",
        part: Part::Tokens,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        if ctx.budgets.skill_description <= 0 {
            return;
        }
        over_budget(
            sink,
            fm.line("description"),
            "description",
            ctx.tokens.description,
            ctx.budgets.skill_description,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::lint::testing::*;

    #[test]
    fn token_budgets() {
        let (_base, target) = write_skill("budgeted", VALID_SKILL);

        let mut opts = generous();
        opts.budgets.skill = 1;
        let res = linter(opts).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec!["name.dir-mismatch", "tokens.content"]
        );

        let mut opts = generous();
        opts.budgets.skill = 0;
        let res = linter(opts).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec!["name.dir-mismatch"]);

        let mut opts = generous();
        opts.budgets.skill_name = 1;
        let res = linter(opts).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec!["name.dir-mismatch", "tokens.name"]
        );

        let mut opts = generous();
        opts.budgets.skill_description = 2;
        let res = linter(opts).file(&target).unwrap();
        assert_eq!(
            rule_ids(&res.findings),
            vec!["name.dir-mismatch", "tokens.description"]
        );

        let res = lint(generous(), &target);
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

        let mut opts = generous();
        opts.budgets.skill = 10000;
        opts.budgets.agents = 1;
        let checker = linter(opts.clone());

        let agents_res = checker.file(&agents).unwrap();
        assert_eq!(agents_res.errors(), 1);

        let skill_res = checker.file(&skill).unwrap();
        assert!(
            !skill_res
                .findings
                .iter()
                .any(|f| f.rule == "tokens.content")
        );

        opts.budgets.agents = 0;
        opts.budgets.skill = 1;
        let skill_res = linter(opts).file(&skill).unwrap();
        assert!(
            skill_res
                .findings
                .iter()
                .any(|f| f.rule == "tokens.content")
        );
    }
}
