//! Turns which rules ran, and which of them fired, into a file's score.

use std::collections::HashSet;

use super::finding::Counts;
use super::ids::*;
use crate::config::Config;
use crate::discover::Kind;

/// The part of a file's score a rule's outcome feeds. Every id in [`RULES`]
/// maps to one, enforced by a test, so a new rule cannot be added without
/// deciding what it rates. Named `Part` rather than the more natural
/// `Component` because `std::path::Component` already holds that name here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Frontmatter,
    Tokens,
    FileReferences,
}

/// Spelled out per rule rather than falling back to a catch-all arm: the
/// fallback is what would let a new rule slip into the wrong component.
pub(super) fn part(rule: &str) -> Option<Part> {
    match rule {
        RULE_FRONTMATTER_MISSING
        | RULE_FRONTMATTER_NOT_FIRST
        | RULE_FRONTMATTER_UNTERMINATED
        | RULE_FRONTMATTER_INVALID
        | RULE_FRONTMATTER_UNKNOWN_KEY
        | RULE_NAME_REQUIRED
        | RULE_NAME_FORMAT
        | RULE_NAME_LENGTH
        | RULE_NAME_DIR_MISMATCH
        | RULE_DESCRIPTION_REQUIRED
        | RULE_DESCRIPTION_LENGTH
        | RULE_ALLOWED_TOOLS_TYPE
        | RULE_METADATA_TYPE => Some(Part::Frontmatter),
        RULE_TOKENS_CONTENT | RULE_TOKENS_NAME | RULE_TOKENS_DESCRIPTION => Some(Part::Tokens),
        RULE_FILE_REFERENCE_MISSING => Some(Part::FileReferences),
        _ => None,
    }
}

/// Which rules ran on a file and which of them fired. Tracked per rule id, not
/// per finding, so a rule that reports repeatedly -- an unknown key on every
/// line, a broken link on every line -- still counts once toward the score.
#[derive(Debug, Default)]
pub(super) struct Tally {
    pub(super) applied: HashSet<&'static str>,
    pub(super) failed: HashSet<&'static str>,
}

impl Tally {
    /// Returns how many of a component's rules ran, and how many of those
    /// fired.
    fn counts(&self, p: Part) -> (usize, usize) {
        let of = |set: &HashSet<&'static str>| set.iter().filter(|r| part(r) == Some(p)).count();
        (of(&self.applied), of(&self.failed))
    }
}

/// Rates one file from 0 to 100: the mean of its frontmatter, token-budget and
/// file-reference components.
///
/// A component with nothing to judge is left out of the mean rather than
/// counted as perfect, so an inapplicable check cannot inflate a score. That
/// covers an AGENTS.md, whose front matter lintmatter deliberately does not
/// validate; a file that names no checkable reference; and a budget switched
/// off. A file with no applicable component at all rates 100: there was
/// nothing to hold against it.
pub(super) fn score(tokens: &Counts, cfg: &Config, kind: Kind, tally: &Tally) -> u8 {
    let mut parts: Vec<f64> = Vec::new();

    let (applied, failed) = tally.counts(Part::Frontmatter);
    if applied > 0 {
        parts.push(100.0 * (applied - failed) as f64 / applied as f64);
    }

    // One sub-score per budget that was actually checked, averaged, so a
    // skill's name and description weigh against its body rather than beside
    // the other two components.
    let budgets: Vec<f64> = [
        (RULE_TOKENS_CONTENT, tokens.content, cfg.content_limit(kind)),
        (RULE_TOKENS_NAME, tokens.name, cfg.max_skill_name_tokens),
        (
            RULE_TOKENS_DESCRIPTION,
            tokens.description,
            cfg.max_skill_description_tokens,
        ),
    ]
    .into_iter()
    .filter(|(rule, _, _)| tally.applied.contains(rule))
    .map(|(_, count, limit)| budget_score(count, limit))
    .collect();
    if !budgets.is_empty() {
        parts.push(budgets.iter().sum::<f64>() / budgets.len() as f64);
    }

    if tally.applied.contains(RULE_FILE_REFERENCE_MISSING) {
        let broken = tally.failed.contains(RULE_FILE_REFERENCE_MISSING);
        parts.push(if broken { 0.0 } else { 100.0 });
    }

    if parts.is_empty() {
        return 100;
    }
    let mean = parts.iter().sum::<f64>() / parts.len() as f64;
    // Floored, so a score only reaches a band it has fully earned: 87.5 rates
    // 87 rather than being rounded up into an 88 the file did not achieve.
    mean.clamp(0.0, 100.0).floor() as u8
}

/// Scores one token budget: full marks at or under the limit, then falling
/// linearly to zero at twice the limit. Graded rather than pass/fail so a file
/// a little over its budget does not rate the same as one five times over.
fn budget_score(count: usize, limit: i64) -> f64 {
    if limit <= 0 || count as i64 <= limit {
        return 100.0;
    }
    let over = (count as i64 - limit) as f64 / limit as f64;
    100.0 * (1.0 - over.min(1.0))
}
