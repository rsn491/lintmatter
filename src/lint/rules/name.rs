//! Rules over a skill's `name`.
//!
//! Every rule past `Required` reads `ctx.name()`, which is `Some` only for a
//! present, non-empty name. That is what keeps them quiet when `name.required`
//! has already fired, without any of them knowing about the others.

use std::sync::LazyLock;

use regex::Regex;

use crate::lint::rule::{Part, Rule, RuleMeta, SKILL_ONLY};
use crate::lint::{FileContext, FindingSink};

/// The skill spec's limit on a name, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// The spec's naming rule: lowercase alphanumerics in hyphen-separated
/// segments.
static NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());

pub struct Required;

impl Rule for Required {
    const META: RuleMeta = RuleMeta {
        id: "name.required",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        sink.applies();
        if ctx.name().is_none() {
            sink.error(
                fm.line("name"),
                "front matter must declare a non-empty string name".to_string(),
            );
        }
    }
}

pub struct Format;

impl Rule for Format {
    const META: RuleMeta = RuleMeta {
        id: "name.format",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let (Some(fm), Some(name)) = (ctx.frontmatter(), ctx.name()) else {
            return;
        };
        sink.applies();
        if !NAME_RE.is_match(name) {
            sink.error(
                fm.line("name"),
                format!(
                    "name {name:?} must be lowercase letters, digits and single hyphens (for example my-skill)"
                ),
            );
        }
    }
}

pub struct Length;

impl Rule for Length {
    const META: RuleMeta = RuleMeta {
        id: "name.length",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let (Some(fm), Some(name)) = (ctx.frontmatter(), ctx.name()) else {
            return;
        };
        sink.applies();
        let n = name.chars().count();
        if n > MAX_NAME_CHARS {
            sink.error(
                fm.line("name"),
                format!("name is {n} characters, over the {MAX_NAME_CHARS} character limit"),
            );
        }
    }
}

pub struct DirMismatch;

impl Rule for DirMismatch {
    const META: RuleMeta = RuleMeta {
        id: "name.dir-mismatch",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let (Some(fm), Some(name)) = (ctx.frontmatter(), ctx.name()) else {
            return;
        };
        if let Some(dir) = ctx.skill_dir() {
            sink.applies();
            if dir != name {
                sink.warn(
                    fm.line("name"),
                    format!("name {name:?} does not match its directory {dir:?}"),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::Linter;
    use crate::lint::testing::*;
    use std::path::Path;

    #[test]
    fn name_rules() {
        let long_name = "a".repeat(MAX_NAME_CHARS + 1);
        assert_skill_cases(vec![
            (
                "name missing",
                "nameless",
                "---\ndescription: A skill with no name.\n---\nBody.\n".to_string(),
                vec!["name.required"],
            ),
            (
                "name empty",
                "empty-name",
                "---\nname: \"\"\ndescription: A skill with an empty name.\n---\nBody.\n".to_string(),
                vec!["name.required"],
            ),
            (
                "name not a scalar",
                "listy-name",
                "---\nname:\n  - listy-name\ndescription: A skill whose name is a list.\n---\nBody.\n"
                    .to_string(),
                vec!["name.required"],
            ),
            (
                "name has bad characters",
                "Bad_Name",
                "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n".to_string(),
                vec!["name.format"],
            ),
            (
                "name has double hyphen",
                "double--hyphen",
                "---\nname: double--hyphen\ndescription: A skill with a doubled hyphen.\n---\nBody.\n"
                    .to_string(),
                vec!["name.format"],
            ),
            (
                "name too long",
                "long",
                format!(
                    "---\nname: {long_name}\ndescription: A skill with a very long name.\n---\nBody.\n"
                ),
                vec!["name.length", "name.dir-mismatch"],
            ),
            (
                "name does not match directory",
                "some-directory",
                "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n"
                    .to_string(),
                vec!["name.dir-mismatch"],
            ),
        ]);
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

        let res = Linter::new(generous(), holding_dir).file(&target).unwrap();
        assert!(res.findings.is_empty(), "{:?}", rule_ids(&res.findings));

        let res = Linter::new(generous(), base.path().to_path_buf())
            .file(&target)
            .unwrap();
        assert_eq!(rule_ids(&res.findings), vec!["name.dir-mismatch"]);
    }
}
