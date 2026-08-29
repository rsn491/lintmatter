//! The machine-readable report.

use std::io::{self, Write};

use crate::discover::Kind;
use crate::lint::{Counts, FileResult, Severity};

use super::{Report, SCHEMA_VERSION, Summary};

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    version: u32,
    files: Vec<JsonFile<'a>>,
    summary: Summary,
}

/// A serialization view of [`FileResult`].
///
/// Findings no longer store their file -- it is always the enclosing result's
/// path -- so the view re-attaches it, keeping the JSON shape it has always
/// had. Field order here is the wire order, so it matches the structs it
/// mirrors. Building a view also means `--quiet` filters while walking instead
/// of cloning whole results to drop their warnings.
#[derive(serde::Serialize)]
struct JsonFile<'a> {
    path: &'a str,
    kind: Kind,
    tokens: &'a Counts,
    score: u8,
    findings: Vec<JsonFinding<'a>>,
}

#[derive(serde::Serialize)]
struct JsonFinding<'a> {
    file: &'a str,
    #[serde(skip_serializing_if = "is_zero")]
    line: usize,
    rule: &'static str,
    severity: Severity,
    message: &'a str,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Writes the whole run as a single object, sorted by path upstream so the
/// output diffs cleanly between runs.
pub struct JsonReporter {
    pub quiet: bool,
}

impl Report for JsonReporter {
    fn render(
        &self,
        w: &mut dyn Write,
        results: &[FileResult],
        summary: &Summary,
    ) -> io::Result<()> {
        let files: Vec<JsonFile> = results
            .iter()
            .map(|r| JsonFile {
                path: &r.path,
                kind: r.kind,
                tokens: &r.tokens,
                score: r.score,
                findings: r
                    .findings
                    .iter()
                    .filter(|f| !self.quiet || f.severity != Severity::Warning)
                    .map(|f| JsonFinding {
                        file: &r.path,
                        line: f.line,
                        rule: f.rule,
                        severity: f.severity,
                        message: &f.message,
                    })
                    .collect(),
            })
            .collect();

        let report = JsonReport {
            version: SCHEMA_VERSION,
            files,
            summary: *summary,
        };
        let mut buf = serde_json::to_vec_pretty(&report).map_err(io::Error::other)?;
        buf.push(b'\n');
        w.write_all(&buf)
    }
}
