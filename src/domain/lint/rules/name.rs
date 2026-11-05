//! Rules over a skill's `name`.
//!
//! Every rule past `Required` reads `ctx.name()`, which is `Some` only for a
//! present, non-empty name. That is what keeps them quiet when `name.required`
//! has already fired, without any of them knowing about the others.

use std::sync::LazyLock;

use regex::Regex;

use crate::domain::lint::rule::Rule;
use crate::domain::lint::{
    FileContext, FindingSink, MAX_NAME_CHARS, RULE_NAME_DIR_MISMATCH, RULE_NAME_FORMAT,
    RULE_NAME_LENGTH, RULE_NAME_REQUIRED,
};

/// The spec's naming rule: lowercase alphanumerics in hyphen-separated
/// segments.
static NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid regex literal"));

pub struct Required;

impl Rule for Required {
    fn id(&self) -> &'static str {
        RULE_NAME_REQUIRED
    }

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
    fn id(&self) -> &'static str {
        RULE_NAME_FORMAT
    }

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
    fn id(&self) -> &'static str {
        RULE_NAME_LENGTH
    }

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
    fn id(&self) -> &'static str {
        RULE_NAME_DIR_MISMATCH
    }

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
