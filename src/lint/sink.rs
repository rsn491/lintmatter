use std::collections::HashSet;

use super::finding::{Finding, Severity};
use super::ids::*;

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
    pub(super) fn counts(&self, p: Part) -> (usize, usize) {
        let of = |set: &HashSet<&'static str>| set.iter().filter(|r| part(r) == Some(p)).count();
        (of(&self.applied), of(&self.failed))
    }
}

/// Collects findings for the rule currently running, applying `--strict`.
///
/// Named for what it does rather than "Reporter", which the `report` module
/// already means. The rule id comes from the sink rather than each call site,
/// so a rule cannot file a finding under someone else's id.
pub struct FindingSink<'a> {
    pub(super) findings: &'a mut Vec<Finding>,
    pub(super) tally: &'a mut Tally,
    pub(super) strict: bool,
    pub(super) rule: &'static str,
}

impl FindingSink<'_> {
    /// Records a violation that should fail the build.
    pub fn error(&mut self, line: usize, msg: String) {
        self.add(Severity::Error, line, msg);
    }

    /// Records a stylistic mismatch. `--strict` promotes it to an error.
    pub fn warn(&mut self, line: usize, msg: String) {
        self.add(Severity::Warning, line, msg);
    }

    /// Records that the current rule had something to judge on this file,
    /// whether or not it goes on to fire. A rule that never calls this --
    /// because its precondition never held, the way `allowed-tools.type`
    /// stays quiet when the key is absent -- leaves the score's numerator and
    /// denominator alike, the same way a disabled rule already does by never
    /// running its `check` at all.
    pub fn applies(&mut self) {
        self.tally.applied.insert(self.rule);
    }

    fn add(&mut self, mut sev: Severity, line: usize, msg: String) {
        // A rule that fired must be in the denominator too, whether or not the
        // check site remembered to call `applies` first.
        self.tally.applied.insert(self.rule);
        self.tally.failed.insert(self.rule);
        if sev == Severity::Warning && self.strict {
            sev = Severity::Error;
        }
        self.findings.push(Finding {
            line,
            rule: self.rule,
            severity: sev,
            message: msg,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the score against a rule being added and rated by accident: a
    /// new id has to be given a part before this passes.
    #[test]
    fn every_rule_belongs_to_a_score_part() {
        for rule in RULES.iter() {
            assert!(part(rule).is_some(), "{rule} is not assigned a score part");
        }
    }
}
