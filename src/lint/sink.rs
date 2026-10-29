//! The channel rules report through.

use super::finding::{Finding, Severity};
use super::score::Tally;

/// Collects findings for the rule currently running, applying `--strict`.
///
/// Named for what it does rather than "Reporter", which the `report` module
/// already means. The rule id comes from the sink rather than each call site,
/// so a rule cannot file a finding under someone else's id.
pub struct FindingSink<'a> {
    findings: &'a mut Vec<Finding>,
    tally: &'a mut Tally,
    strict: bool,
    path: &'a str,
    rule: &'static str,
}

impl<'a> FindingSink<'a> {
    pub(super) fn new(
        findings: &'a mut Vec<Finding>,
        tally: &'a mut Tally,
        strict: bool,
        path: &'a str,
    ) -> Self {
        FindingSink {
            findings,
            tally,
            strict,
            path,
            rule: "",
        }
    }

    /// Names the rule about to run, so everything it files carries its id.
    pub(super) fn set_rule(&mut self, rule: &'static str) {
        self.rule = rule;
    }
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
            file: self.path.to_string(),
            line,
            rule: self.rule.to_string(),
            severity: sev,
            message: msg,
        });
    }
}
