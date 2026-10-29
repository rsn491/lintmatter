//! Rules over a skill's `description`.

use crate::lint::rule::Rule;
use crate::lint::rules::humanize;
use crate::lint::{
    FileContext, FindingSink, MAX_DESCRIPTION_CHARS, RULE_DESCRIPTION_LENGTH,
    RULE_DESCRIPTION_REQUIRED,
};

pub struct Required;

impl Rule for Required {
    fn id(&self) -> &'static str {
        RULE_DESCRIPTION_REQUIRED
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        sink.applies();
        if ctx.description().is_none() {
            sink.error(
                fm.line("description"),
                "front matter must declare a non-empty string description saying when to use the skill".to_string(),
            );
        }
    }
}

pub struct Length;

impl Rule for Length {
    fn id(&self) -> &'static str {
        RULE_DESCRIPTION_LENGTH
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let (Some(fm), Some(desc)) = (ctx.frontmatter(), ctx.description()) else {
            return;
        };
        sink.applies();
        let n = desc.chars().count();
        if n > MAX_DESCRIPTION_CHARS {
            sink.error(
                fm.line("description"),
                format!(
                    "description is {} characters, over the {} character limit",
                    humanize(n as i64),
                    humanize(MAX_DESCRIPTION_CHARS as i64)
                ),
            );
        }
    }
}
