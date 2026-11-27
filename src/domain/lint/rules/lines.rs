//! The SKILL.md body's line budget.
//!
//! Anthropic advises keeping a skill's body under 500 lines and moving detail
//! into referenced files, which load only when needed. Line count is a cruder
//! measure than tokens, so going over it is a warning, not an error.

use crate::domain::lint::rule::Rule;
use crate::domain::lint::{FileContext, FindingSink, RULE_LINES_CONTENT};
use crate::utils::humanize;

pub struct Content;

impl Rule for Content {
    fn id(&self) -> &'static str {
        RULE_LINES_CONTENT
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let limit = ctx.cfg.max_skill_lines;
        if limit <= 0 {
            return;
        }
        sink.applies();
        let got = ctx.tokens.lines;
        if got as i64 <= limit {
            return;
        }
        sink.warn(
            0,
            format!(
                "content is {} lines, over the {} line limit: move detail into referenced files",
                humanize(got as i64),
                humanize(limit)
            ),
        );
    }
}
