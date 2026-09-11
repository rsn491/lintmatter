//! Rules over the front-matter block itself: whether it is there, parseable,
//! and made only of keys the spec defines.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::discover::Kind;
use crate::lint::rule::Rule;
use crate::lint::{
    FileContext, FindingSink, RULE_FRONTMATTER_INVALID, RULE_FRONTMATTER_MISSING,
    RULE_FRONTMATTER_NOT_FIRST, RULE_FRONTMATTER_UNKNOWN_KEY, RULE_FRONTMATTER_UNTERMINATED,
};
use crate::parse::ErrKind;

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
    fn id(&self) -> &'static str {
        RULE_FRONTMATTER_MISSING
    }

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
    fn id(&self) -> &'static str {
        RULE_FRONTMATTER_NOT_FIRST
    }

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
    fn id(&self) -> &'static str {
        RULE_FRONTMATTER_UNTERMINATED
    }

    fn applies_to(&self, _kind: Kind) -> bool {
        true
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        if let Some(e) = ctx.error_of(ErrKind::Unterminated) {
            sink.error(e.line, e.msg.clone());
        }
    }
}

pub struct Invalid;

impl Rule for Invalid {
    fn id(&self) -> &'static str {
        RULE_FRONTMATTER_INVALID
    }

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
    fn id(&self) -> &'static str {
        RULE_FRONTMATTER_UNKNOWN_KEY
    }

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
