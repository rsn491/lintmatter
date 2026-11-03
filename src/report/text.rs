//! The human-readable report.

use std::io::{self, Write};

use crate::lint::{FileResult, Finding, Severity};
use crate::utils::plural;

use super::Summary;
use super::reporter::{Report, filter_errors};

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const ORANGE: &str = "\x1b[38;5;208m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

const ERROR_EMOJI: &str = "\u{274c}"; // ❌
const WARNING_EMOJI: &str = "\u{26a0}\u{fe0f}"; // ⚠️

/// The color a score bands into: green is healthy, yellow wants a look,
/// orange is a real problem, red wants work.
fn score_color(score: u8) -> &'static str {
    match score {
        90..=100 => GREEN,
        70..=89 => YELLOW,
        50..=69 => ORANGE,
        _ => RED,
    }
}

/// The band label a score falls into, mirroring the web UI's score card
/// (see web/src/index.html's `SCORE_BANDS`) so a run rates the same in both
/// places.
fn score_band_label(score: u8) -> &'static str {
    match score {
        90..=100 => "Healthy",
        70..=89 => "Worth a look",
        50..=69 => "Needs work",
        _ => "Wants work",
    }
}

/// Colors a score by band so a report scans at a glance.
fn paint_score(color: bool, score: u8) -> String {
    paint(color, score_color(score), &score.to_string())
}

/// Wraps `s` in an SGR color code when `color` is set; otherwise returns it
/// unchanged.
fn paint(color: bool, code: &str, s: &str) -> String {
    if color {
        format!("{code}{s}{RESET}")
    } else {
        s.to_string()
    }
}

/// A rough per-character terminal column width: emoji render as two columns
/// and a variation selector (the invisible marker on `WARNING_EMOJI` that
/// picks its emoji-style glyph) renders as zero. `chars().count()` alone
/// would size the scorecard border a column short wherever an emoji lands,
/// visibly misaligning its right edge; this is enough to size it correctly
/// for the handful of symbols this reporter prints, without pulling in a
/// full Unicode width table for one box border.
fn visible_width(s: &str) -> usize {
    s.chars()
        .map(|c| match c as u32 {
            0xfe00..=0xfe0f => 0,
            0x2600..=0x27bf | 0x1f300..=0x1faff => 2,
            _ => 1,
        })
        .sum()
}

/// Groups findings under a header naming their file and its score, so the
/// path is not repeated on every line. A file with a perfect score is
/// dropped entirely -- score 100 means no findings, so there is nothing to
/// report -- the same way `--quiet` drops any file with nothing left to
/// report. A blank line separates file groups and sets the final summary
/// line apart.
///
/// When `color` is set, severities are colorized and prefixed with a symbol;
/// otherwise the output is plain text, unchanged from earlier versions so it
/// stays friendly to grep and diffing.
pub struct TextReporter {
    pub quiet: bool,
    pub color: bool,
}

impl Report for TextReporter {
    fn render(
        &self,
        w: &mut dyn Write,
        results: &[FileResult],
        summary: &Summary,
    ) -> io::Result<()> {
        let color = self.color;
        for r in results {
            let owned;
            let findings: &[Finding] = if self.quiet {
                owned = filter_errors(&r.findings);
                &owned
            } else {
                &r.findings
            };
            if self.quiet && findings.is_empty() {
                continue;
            }
            if r.score == 100 {
                continue;
            }
            writeln!(
                w,
                "{}  {}",
                paint(color, BOLD, &r.path),
                paint_score(color, r.score)
            )?;
            for f in findings {
                let location = if f.line > 0 {
                    format!("{}: ", f.line)
                } else {
                    String::new()
                };
                let (emoji, code) = match f.severity {
                    Severity::Error => (ERROR_EMOJI, RED),
                    Severity::Warning => (WARNING_EMOJI, YELLOW),
                };
                let severity = paint(color, code, &f.severity.to_string());
                if color {
                    writeln!(
                        w,
                        "  {location}{emoji} {severity}: {}: {}",
                        f.rule, f.message
                    )?;
                } else {
                    writeln!(w, "  {location}{severity}: {}: {}", f.rule, f.message)?;
                }
            }
            writeln!(w)?;
        }

        write_scorecard(w, color, summary)
    }
}

/// Prints the run's score as a bordered card -- the score and its band on
/// one line, then a blank line setting it apart from the file counts
/// underneath. The border is sized off each line's plain text, since ANSI
/// codes and box-drawing padding must be computed independently of any
/// color applied within a line.
fn write_scorecard(w: &mut dyn Write, color: bool, s: &Summary) -> io::Result<()> {
    let checked = plural(s.files, "file", "files");
    let errors = plural(s.files_with_errors, "file with errors", "files with errors");
    let warnings = plural(
        s.files_with_warnings,
        "file with warnings",
        "files with warnings",
    );
    let clean = s.files_with_errors == 0 && s.files_with_warnings == 0;

    let band = score_band_label(s.score);
    let header_plain = format!("Score: {}/100 \u{b7} {band}", s.score);
    let meta_plain = format!("{checked} checked, {errors}, {warnings}");

    let header_out = if color {
        format!(
            "Score: {}/100 \u{b7} {}",
            paint_score(color, s.score),
            paint(color, score_color(s.score), band)
        )
    } else {
        header_plain.clone()
    };
    let errors_out = if s.files_with_errors > 0 {
        paint(color, RED, &errors)
    } else {
        errors
    };
    let warnings_out = if s.files_with_warnings > 0 {
        paint(color, YELLOW, &warnings)
    } else {
        warnings
    };
    let checked_out = if clean {
        paint(color, GREEN, &checked)
    } else {
        checked
    };
    let meta_out = format!("{checked_out} checked, {errors_out}, {warnings_out}");

    let width = visible_width(&header_plain).max(visible_width(&meta_plain));
    let border = "─".repeat(width + 2);
    let pad = |plain: &str| " ".repeat(width - visible_width(plain));

    writeln!(w, "┌{border}┐")?;
    writeln!(w, "│ {header_out}{} │", pad(&header_plain))?;
    writeln!(w, "│{}│", " ".repeat(width + 2))?;
    writeln!(w, "│ {meta_out}{} │", pad(&meta_plain))?;
    writeln!(w, "└{border}┘")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Kind;
    use crate::lint::Counts;
    use crate::report::summarize;
    use crate::report::test_utils::results;

    fn render_text(results: &[FileResult], quiet: bool, color: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        TextReporter { quiet, color }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
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
}
