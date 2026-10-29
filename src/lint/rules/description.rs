//! Rules over a skill's `description`.

use crate::lint::rule::{Part, Rule, RuleMeta, SKILL_ONLY};
use crate::lint::rules::humanize;
use crate::lint::{FileContext, FindingSink};

/// The skill spec's limit on a description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 1024;

pub struct Required;

impl Rule for Required {
    const META: RuleMeta = RuleMeta {
        id: "description.required",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

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
    const META: RuleMeta = RuleMeta {
        id: "description.length",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::testing::*;

    #[test]
    fn description_rules() {
        let long_desc = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
        assert_skill_cases(vec![
            (
                "description missing",
                "no-description",
                "---\nname: no-description\n---\nBody.\n".to_string(),
                vec!["description.required"],
            ),
            (
                "description empty",
                "blank-description",
                "---\nname: blank-description\ndescription: \"   \"\n---\nBody.\n".to_string(),
                vec!["description.required"],
            ),
            (
                "description too long",
                "verbose",
                format!("---\nname: verbose\ndescription: {long_desc}\n---\nBody.\n"),
                vec!["description.length"],
            ),
        ]);
    }
}
