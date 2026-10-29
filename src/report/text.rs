//! The human-readable report.

use std::io::{self, Write};

use super::format::plural;
use crate::model::{FileResult, Finding, Severity};

use super::{Report, Summary, filter_errors};

pub(super) const RED: &str = "\x1b[31m";
pub(super) const YELLOW: &str = "\x1b[33m";
pub(super) const ORANGE: &str = "\x1b[38;5;208m";
pub(super) const GREEN: &str = "\x1b[32m";
pub(super) const BOLD: &str = "\x1b[1m";
pub(super) const RESET: &str = "\x1b[0m";

pub(super) const ERROR_EMOJI: &str = "\u{274c}"; // ❌
pub(super) const WARNING_EMOJI: &str = "\u{26a0}\u{fe0f}"; // ⚠️

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
pub(super) fn score_band_label(score: u8) -> &'static str {
    match score {
        90..=100 => "Healthy",
        70..=89 => "Worth a look",
        50..=69 => "Needs work",
        _ => "Wants work",
    }
}

/// Colors a score by band so a report scans at a glance.
pub(super) fn paint_score(color: bool, score: u8) -> String {
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
