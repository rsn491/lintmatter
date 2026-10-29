//! Rules over the front-matter block itself: whether it is there, parseable,
//! and made only of keys the spec defines.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::lint::rule::{ALL_KINDS, Part, Rule, RuleMeta, SKILL_ONLY};
use crate::lint::{FileContext, FindingSink};
use crate::source::parse::ErrKind;

/// The front-matter keys the skill spec defines, plus the Claude Code
/// extensions lintmatter additionally supports. Anything else is reported as an
/// unknown key.
static KNOWN_KEYS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "name",
        "description",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
        "disable-model-invocation",
        "argument-hint",
    ]
    .into_iter()
    .collect()
});

pub struct Missing;

impl Rule for Missing {
    const META: RuleMeta = RuleMeta {
        id: "frontmatter.missing",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        if ctx.doc.error.is_some() {
            return;
        }
        sink.applies();
        if ctx.doc.frontmatter.present {
            return;
        }
        sink.error(
            1,
            "missing YAML front matter: a skill must open with a --- fenced block declaring name and description".to_string(),
        );
    }
}

pub struct NotFirst;

impl Rule for NotFirst {
    const META: RuleMeta = RuleMeta {
        id: "frontmatter.not-first",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        if let Some(e) = ctx.error_of(ErrKind::NotFirst) {
            sink.error(e.line, e.msg.clone());
        }
    }
}

/// Reported for AGENTS.md too: an unclosed fence swallows the document
/// whatever the file is, so it is not a skill-spec question.
pub struct Unterminated;

impl Rule for Unterminated {
    const META: RuleMeta = RuleMeta {
        id: "frontmatter.unterminated",
        part: Part::Frontmatter,
        kinds: ALL_KINDS,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        if let Some(e) = ctx.error_of(ErrKind::Unterminated) {
            sink.error(e.line, e.msg.clone());
        }
    }
}

pub struct Invalid;

impl Rule for Invalid {
    const META: RuleMeta = RuleMeta {
        id: "frontmatter.invalid",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(e) = &ctx.doc.error else { return };
        if matches!(e.kind, ErrKind::NotFirst | ErrKind::Unterminated) {
            return;
        }
        sink.error(e.line, e.msg.clone());
    }
}

pub struct UnknownKey;

impl Rule for UnknownKey {
    const META: RuleMeta = RuleMeta {
        id: "frontmatter.unknown-key",
        part: Part::Frontmatter,
        kinds: SKILL_ONLY,
    };

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let Some(fm) = ctx.frontmatter() else { return };
        sink.applies();
        for key in fm.keys() {
            if !KNOWN_KEYS.contains(key.as_str()) {
                sink.warn(fm.line(key), format!("unknown front-matter key {key:?}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lint::testing::*;

    #[test]
    fn frontmatter_rules() {
        assert_skill_cases(vec![
            (
                "missing front matter",
                "no-frontmatter",
                "# A skill with no front matter\n\nBody.\n".to_string(),
                vec!["frontmatter.missing"],
            ),
            (
                "front matter not first",
                "late-frontmatter",
                "\n---\nname: late-frontmatter\ndescription: Late.\n---\n".to_string(),
                vec!["frontmatter.not-first"],
            ),
            (
                "unterminated front matter",
                "unterminated",
                "---\nname: unterminated\ndescription: No closing fence.\n".to_string(),
                vec!["frontmatter.unterminated"],
            ),
            (
                "invalid yaml",
                "bad-yaml",
                "---\nname: bad-yaml\n  description: wrong indent\n---\nBody.\n".to_string(),
                vec!["frontmatter.invalid"],
            ),
            (
                "front matter is a sequence",
                "sequence",
                "---\n- name: sequence\n---\nBody.\n".to_string(),
                vec!["frontmatter.invalid"],
            ),
            (
                "unknown key",
                "extra-keys",
                "---\nname: extra-keys\ndescription: A skill with a stray key.\nauthor: someone\n---\nBody.\n"
                    .to_string(),
                vec!["frontmatter.unknown-key"],
            ),
        ]);
    }

    #[test]
    fn agents_unterminated_frontmatter_is_reported() {
        let base = tempfile::tempdir().unwrap();
        let src = "---\ntitle: Project instructions\n\n# Instructions never closed\n";
        let target = agents_target(base.path(), src);

        let res = lint(generous(), &target);
        assert_eq!(rule_ids(&res.findings), vec!["frontmatter.unterminated"]);
    }
}
