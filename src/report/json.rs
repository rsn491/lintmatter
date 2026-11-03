//! The machine-readable report.

use std::io::{self, Write};

use crate::discover::Kind;
use crate::lint::{Counts, FileResult, Severity};

use super::Summary;
use super::reporter::Report;

/// Bumped when the JSON shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    version: u32,
    files: Vec<JsonFile<'a>>,
    summary: Summary,
}

/// Serialization view of [`FileResult`]: re-attaches `file` to each finding to
/// keep the wire format, and filters `--quiet` warnings without cloning.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::summarize;
    use crate::report::test_utils::results;

    fn render_json(results: &[FileResult], quiet: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        JsonReporter { quiet }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
    }

    #[test]
    fn json_output_shape() {
        let buf = render_json(&results(), false);
        let got: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(got["version"], SCHEMA_VERSION);
        assert_eq!(got["files"].as_array().unwrap().len(), 2);
        assert_eq!(got["summary"]["files"], 2);
        assert_eq!(got["summary"]["files_with_errors"], 1);
        assert_eq!(got["summary"]["files_with_warnings"], 1);
        assert_eq!(got["summary"]["score"], 73);
        assert_eq!(got["files"][0]["score"], 50);
        assert_eq!(got["files"][1]["score"], 96);
        assert_eq!(got["files"][1]["tokens"]["name"], 2);
        assert!(!String::from_utf8(buf).unwrap().contains("\"line\": 0"));
    }

    #[test]
    fn json_quiet_suppresses_warnings() {
        let buf = render_json(&results(), true);
        let got: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        for file in got["files"].as_array().unwrap() {
            for finding in file["findings"].as_array().unwrap() {
                assert_ne!(finding["severity"], "warning");
            }
        }
        assert_eq!(got["summary"]["files_with_warnings"], 1);
    }

    #[test]
    fn json_quiet_does_not_mutate_input() {
        let input = results();
        let buf = render_json(&input, true);

        // The rendered report drops the warning, but the caller's results are
        // left intact -- quiet is a view, not an edit.
        let got: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        for file in got["files"].as_array().unwrap() {
            assert!(file["findings"].as_array().unwrap().is_empty() || file["path"] == "AGENTS.md");
        }
        assert_eq!(got["summary"]["files_with_warnings"], 1);
        assert_eq!(summarize(&input).files_with_warnings, 1);
        assert_eq!(input[1].findings.len(), 1);
    }
}
