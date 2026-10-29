//! The interface every output format implements.

use std::io::{self, Write};

use crate::lint::{FileResult, Finding, Severity};

use super::Summary;

/// One way of rendering a run.
///
/// Each implementor owns its own switches, so a call site names what it wants
/// once at construction instead of passing a row of positional bools that read
/// as `text(w, &results, false, true)` at the point of use. `w` is a trait
/// object so the reporters can be held as `dyn Report`.
pub trait Report {
    fn render(
        &self,
        w: &mut dyn Write,
        results: &[FileResult],
        summary: &Summary,
    ) -> io::Result<()>;
}

/// Drops warnings, for `--quiet`.
pub(super) fn filter_errors(findings: &[Finding]) -> Vec<Finding> {
    findings
        .iter()
        .filter(|f| f.severity != Severity::Warning)
        .cloned()
        .collect()
}
