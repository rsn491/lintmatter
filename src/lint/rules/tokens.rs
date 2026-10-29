//! Token budget rules.
//!
//! The counts themselves are measured once by the linter before any rule runs;
//! these rules only compare them against the configured budgets. A budget of
//! zero disables its check.

use crate::lint::rule::Rule;
use crate::lint::rules::humanize;
use crate::lint::{
    FileContext, FindingSink, RULE_TOKENS_CONTENT, RULE_TOKENS_DESCRIPTION, RULE_TOKENS_NAME,
};
use crate::model::Kind;

fn over_budget(sink: &mut FindingSink<'_>, line: usize, what: &str, got: usize, limit: i64) {
    if limit <= 0 || got as i64 <= limit {
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
    fn id(&self) -> &'static str {
        RULE_TOKENS_CONTENT
    }

    fn applies_to(&self, _kind: Kind) -> bool {
        true
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let limit = ctx.cfg.content_limit(ctx.target.kind);
        if limit <= 0 {
            return;
        }
        sink.applies();
        over_budget(sink, 0, "content", ctx.tokens.content, limit);
    }
}

pub struct Name;

impl Rule for Name {
    fn id(&self) -> &'static str {
        RULE_TOKENS_NAME
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        if ctx.cfg.max_skill_name_tokens <= 0 {
            return;
        }
        sink.applies();
        over_budget(
            sink,
            fm.line("name"),
            "name",
            ctx.tokens.name,
            ctx.cfg.max_skill_name_tokens,
        );
    }
}

pub struct Description;

impl Rule for Description {
    fn id(&self) -> &'static str {
        RULE_TOKENS_DESCRIPTION
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        if ctx.cfg.max_skill_description_tokens <= 0 {
            return;
        }
        sink.applies();
        over_budget(
            sink,
            fm.line("description"),
            "description",
            ctx.tokens.description,
            ctx.cfg.max_skill_description_tokens,
        );
    }
}
