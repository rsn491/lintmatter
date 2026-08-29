//! Renders lint results for humans and for machines.

mod json;
mod text;

use std::io::{self, Write};

use crate::lint::{FileResult, Finding, Severity};

pub use json::JsonReporter;
pub use text::TextReporter;

/// Bumped when the JSON shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

/// Counts a whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub files: usize,
    pub files_with_errors: usize,
    pub files_with_warnings: usize,
    /// The mean of the per-file scores, so the run's rating is exactly the
    /// average of the numbers printed above it. An empty run rates 100.
    pub score: u8,
}

/// Counts, across results, how many files have at least one error or
/// warning. Mirrors other linters' summaries, which report the files
/// affected rather than a raw finding tally.
pub fn summarize(results: &[FileResult]) -> Summary {
    let mut s = Summary {
        files: results.len(),
        ..Default::default()
    };
    let mut total: u32 = 0;
    for r in results {
        if r.errors() > 0 {
            s.files_with_errors += 1;
        }
        if r.warnings() > 0 {
            s.files_with_warnings += 1;
        }
        total += u32::from(r.score);
    }
    s.score = if results.is_empty() {
        100
    } else {
        // Floored, matching the per-file scores it averages.
        (total / results.len() as u32) as u8
    };
    s
}

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
fn filter_errors(findings: &[Finding]) -> Vec<Finding> {
    findings
        .iter()
        .filter(|f| f.severity != Severity::Warning)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Kind;
    use crate::lint::Counts;

    use super::text::{
        BOLD, ERROR_EMOJI, GREEN, ORANGE, RED, RESET, WARNING_EMOJI, YELLOW, paint_score,
    };

    fn render_text(results: &[FileResult], quiet: bool, color: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        TextReporter { quiet, color }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
    }

    fn render_json(results: &[FileResult], quiet: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        JsonReporter { quiet }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
    }

    fn results() -> Vec<FileResult> {
        vec![
            FileResult {
                path: "AGENTS.md".to_string(),
                kind: Kind::Agents,
                tokens: Counts {
                    content: 6142,
                    name: 0,
                    description: 0,
                },
                score: 50,
                findings: vec![Finding {
                    line: 0,
                    rule: crate::lint::RULE_TOKENS_CONTENT,
                    severity: Severity::Error,
                    message: "content is 6,142 tokens, over the 5,000 token limit".to_string(),
                }],
            },
            FileResult {
                path: "skills/thing/SKILL.md".to_string(),
                kind: Kind::Skill,
                tokens: Counts {
                    content: 120,
                    name: 2,
                    description: 30,
                },
                score: 96,
                findings: vec![Finding {
                    line: 2,
                    rule: crate::lint::RULE_NAME_DIR_MISMATCH,
                    severity: Severity::Warning,
                    message: "name \"other\" does not match its directory \"thing\"".to_string(),
                }],
            },
        ]
    }

    #[test]
    fn summarize_totals_across_results() {
        assert_eq!(
            summarize(&results()),
            Summary {
                files: 2,
                files_with_errors: 1,
                files_with_warnings: 1,
                score: 73,
            }
        );
    }

    #[test]
    fn text_output_shape() {
        let buf = render_text(&results(), false, false);
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 11, "{out}");
        assert_eq!(lines[0], "AGENTS.md  50");
        assert!(
            lines[1].starts_with("  error: tokens.content: "),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2], "");
        assert_eq!(lines[3], "skills/thing/SKILL.md  96");
        assert!(
            lines[4].starts_with("  2: warning: name.dir-mismatch: "),
            "{}",
            lines[4]
        );
        assert_eq!(lines[5], "");
        // The run's scorecard: a bordered box with the score on top, a blank
        // line, then the file counts underneath.
        assert!(
            lines[6].starts_with('┌') && lines[6].ends_with('┐'),
            "{out}"
        );
        assert!(lines[7].contains("Score: 73/100 · Worth a look"), "{out}");
        assert!(
            lines[8].starts_with('│')
                && lines[8].ends_with('│')
                && lines[8].trim_matches('│').trim().is_empty(),
            "{out}"
        );
        assert!(
            lines[9].contains("2 files checked, 1 file with errors, 1 file with warnings"),
            "{out}"
        );
        assert!(
            lines[10].starts_with('└') && lines[10].ends_with('┘'),
            "{out}"
        );
    }

    /// A file with a perfect score is dropped entirely -- there's nothing to
    /// flag -- in both the normal and `--quiet` views.
    #[test]
    fn text_drops_files_with_a_perfect_score() {
        let clean = vec![FileResult {
            path: "skills/good/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 100,
            findings: vec![],
        }];
        let out = String::from_utf8(render_text(&clean, false, false)).unwrap();
        assert!(!out.contains("skills/good/SKILL.md"), "{out}");
        assert!(out.contains("Score: 100/100 · Healthy"), "{out}");

        let out = String::from_utf8(render_text(&clean, true, false)).unwrap();
        assert!(!out.contains("skills/good/SKILL.md"), "{out}");
    }

    #[test]
    fn text_quiet_suppresses_warnings() {
        let buf = render_text(&results(), true, false);
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains("warning: "));
        assert!(out.contains("1 file with warnings"));
    }

    #[test]
    fn text_singular_plural() {
        let buf = render_text(&[], false, false);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("Score: 100/100 · Healthy"), "{out}");
        assert!(
            out.contains("0 files checked, 0 files with errors, 0 files with warnings"),
            "{out}"
        );
    }

    #[test]
    fn text_color_adds_symbols_and_sgr_codes() {
        let buf = render_text(&results(), false, true);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains(ERROR_EMOJI), "{out}");
        assert!(out.contains(WARNING_EMOJI), "{out}");
        assert!(out.contains(RED), "{out}");
        assert!(out.contains(YELLOW), "{out}");
        assert!(out.contains(BOLD), "{out}");
        assert!(out.contains(RESET), "{out}");
        assert!(out.contains("error"), "{out}");
        assert!(out.contains("warning"), "{out}");
    }

    #[test]
    fn text_color_clean_run_shows_green_count() {
        let buf = render_text(&[], false, true);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains(GREEN), "{out}");
        assert!(!out.contains(RED), "{out}");
        assert!(!out.contains(YELLOW), "{out}");
    }

    /// A mid-range score bad enough to fall short of yellow still isn't the
    /// worst band: it's painted orange rather than red.
    #[test]
    fn paint_score_orange_band() {
        assert_eq!(paint_score(true, 60), format!("{ORANGE}60{RESET}"));
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
