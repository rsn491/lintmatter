//! The machine-readable report. The JSON shape is defined here and nowhere
//! else: the domain types carry no serialization attributes, so the wire
//! format cannot change by accident when the model does.

use std::io::{self, Write};

use crate::domain::{Counts, FileResult, Kind, Severity, Summary};

use super::reporter::Report;

/// Bumped when the JSON shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    version: u32,
    files: Vec<JsonFile<'a>>,
    summary: JsonSummary,
}

/// Serialization view of [`FileResult`]: re-attaches `file` to each finding to
/// keep the wire format, and filters `--quiet` warnings without cloning.
#[derive(serde::Serialize)]
struct JsonFile<'a> {
    path: &'a str,
    kind: &'static str,
    tokens: JsonCounts,
    score: u8,
    findings: Vec<JsonFinding<'a>>,
}

/// A zero name or description count means there was nothing to count (AGENTS.md
/// has neither), so the key is omitted rather than reported as a cost of 0.
#[derive(serde::Serialize)]
struct JsonCounts {
    content: usize,
    #[serde(skip_serializing_if = "is_zero")]
    name: usize,
    #[serde(skip_serializing_if = "is_zero")]
    description: usize,
}

impl From<&Counts> for JsonCounts {
    fn from(c: &Counts) -> Self {
        JsonCounts {
            content: c.content,
            name: c.name,
            description: c.description,
        }
    }
}

#[derive(serde::Serialize)]
struct JsonFinding<'a> {
    file: &'a str,
    /// Zero marks a finding about the file as a whole; it has no line to show.
    #[serde(skip_serializing_if = "is_zero")]
    line: usize,
    rule: &'static str,
    severity: &'static str,
    message: &'a str,
}

#[derive(serde::Serialize)]
struct JsonSummary {
    files: usize,
    files_with_errors: usize,
    files_with_warnings: usize,
    score: u8,
}

impl From<&Summary> for JsonSummary {
    fn from(s: &Summary) -> Self {
        JsonSummary {
            files: s.files,
            files_with_errors: s.files_with_errors,
            files_with_warnings: s.files_with_warnings,
            score: s.score,
        }
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Spelled out rather than borrowed from `Display`, so rewording the text
/// report can never change the JSON schema.
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Agents => "agents",
        Kind::Skill => "skill",
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
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
                kind: kind_name(r.kind),
                tokens: JsonCounts::from(&r.tokens),
                score: r.score,
                findings: r
                    .findings
                    .iter()
                    .filter(|f| !self.quiet || f.severity != Severity::Warning)
                    .map(|f| JsonFinding {
                        file: &r.path,
                        line: f.line,
                        rule: f.rule,
                        severity: severity_name(f.severity),
                        message: &f.message,
                    })
                    .collect(),
            })
            .collect();

        let report = JsonReport {
            version: SCHEMA_VERSION,
            files,
            summary: JsonSummary::from(summary),
        };
        let mut buf = serde_json::to_vec_pretty(&report).map_err(io::Error::other)?;
        buf.push(b'\n');
        w.write_all(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::summarize;
    use crate::domain::test_utils::results;

    fn render_json(results: &[FileResult], quiet: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        JsonReporter { quiet }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
    }

    #[test]
    fn json_output_shape() {
        // Given
        let input = results();

        // When
        let buf = render_json(&input, false);

        // Then
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
        // Given
        let input = results();

        // When
        let buf = render_json(&input, true);

        // Then
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
        // Given
        let input = results();

        // When
        let buf = render_json(&input, true);

        // Then
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
