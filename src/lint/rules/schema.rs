//! Type rules over the optional front-matter keys.

use crate::lint::rule::{Part, Rule, RuleMeta, SKILL_ONLY};
use crate::lint::{FileContext, FindingSink};
use crate::source::parse::Value;

pub struct AllowedToolsType;

impl Rule for AllowedToolsType {
    const META: RuleMeta = RuleMeta {
        id: "allowed-tools.type",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        if !fm.has("allowed-tools") {
            return;
        }
        sink.applies();
        if fm.string_slice("allowed-tools").is_none() {
            sink.error(
                fm.line("allowed-tools"),
                "allowed-tools must be a list of tool names or a comma-separated string"
                    .to_string(),
            );
        }
    }
}

pub struct MetadataType;

impl Rule for MetadataType {
    const META: RuleMeta = RuleMeta {
        id: "metadata.type",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        let Some(node) = fm.node("metadata") else {
            return;
        };
        sink.applies();
        if !matches!(node, Value::Mapping) {
            sink.error(
                fm.line("metadata"),
                "metadata must be a mapping of keys to values".to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lint::testing::*;

    #[test]
    fn schema_rules() {
        assert_skill_cases(vec![
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
                vec!["allowed-tools.type"],
            ),
            (
                "metadata wrong type",
                "bad-metadata",
                "---\nname: bad-metadata\ndescription: A skill with scalar metadata.\nmetadata: not-a-mapping\n---\nBody.\n".to_string(),
                vec!["metadata.type"],
            ),
        ]);
    }
}
