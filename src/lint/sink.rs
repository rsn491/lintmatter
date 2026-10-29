//! Where rules file their findings, and the per-rule record the score is
//! computed from.

use crate::model::{Finding, Severity};

use super::rule::{Part, RuleMeta};

/// How one rule fared on one file.
#[derive(Debug)]
pub(super) struct Outcome {
    pub(super) id: &'static str,
    pub(super) part: Part,
    pub(super) failed: bool,
    /// A score out of 100 for a rule rated on a scale rather than pass/fail.
    pub(super) grade: Option<f64>,
}

impl Outcome {
    pub(super) fn score(&self) -> f64 {
        match self.grade {
            Some(g) => g,
            None if self.failed => 0.0,
            None => 100.0,
        }
    }
}

/// Which rules ran on a file and how each fared. Kept per rule rather than
/// per finding, so a rule that reports repeatedly -- an unknown key on every
/// line, a broken link on every line -- still counts once toward the score.
/// In run order, so the score sums in the same order every time.
#[derive(Debug, Default)]
pub(super) struct Tally {
    pub(super) outcomes: Vec<Outcome>,
}

impl Tally {
    fn outcome(&mut self, meta: &RuleMeta) -> &mut Outcome {
        let i = match self.outcomes.iter().position(|o| o.id == meta.id) {
            Some(i) => i,
            None => {
                self.outcomes.push(Outcome {
                    id: meta.id,
                    part: meta.part,
                    failed: false,
                    grade: None,
                });
                self.outcomes.len() - 1
            }
        };
        &mut self.outcomes[i]
    }
}

/// Collects findings for the rule currently running.
///
/// Named for what it does rather than "Reporter", which the `report` module
/// already means. The rule id comes from the sink rather than each call site,
/// so a rule cannot file a finding under someone else's id. Severities are
/// recorded as the rule meant them; `--strict` is the CLI's to apply.
pub struct FindingSink<'a> {
    findings: &'a mut Vec<Finding>,
    tally: &'a mut Tally,
    rule: &'static RuleMeta,
}

impl<'a> FindingSink<'a> {
    pub(super) fn new(
        findings: &'a mut Vec<Finding>,
        tally: &'a mut Tally,
        rule: &'static RuleMeta,
    ) -> Self {
        FindingSink {
            findings,
            tally,
            rule,
        }
    }

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
        self.tally.outcome(self.rule);
    }

    /// Rates the current rule on a scale of 0 to 100 instead of pass/fail,
    /// for a check where being a little off should not cost as much as being
    /// far off. Implies [`Self::applies`].
    pub fn grade(&mut self, score: f64) {
        self.tally.outcome(self.rule).grade = Some(score);
    }

    fn add(&mut self, severity: Severity, line: usize, msg: String) {
        // A rule that fired must be in the denominator too, whether or not the
        // check site remembered to call `applies` first.
        self.tally.outcome(self.rule).failed = true;
        self.findings.push(Finding {
            line,
            rule: self.rule.id,
            severity,
            message: msg,
        });
    }
}
