//! The human-readable report.

use std::io::{self, Write};

use crate::domain::{FileResult, Finding, Severity};
use crate::utils::plural;

use super::reporter::{Report, filter_errors};
use crate::domain::Summary;

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const ORANGE: &str = "\x1b[38;5;208m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
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

/// Ten-cell meter for a score, filled in its band color, so files rank at a
/// glance down a long report.
fn score_bar(score: u8) -> String {
    let filled = ((usize::from(score) + 5) / 10).min(10);
    format!(
        "{}{}",
        paint(true, score_color(score), &"\u{2588}".repeat(filled)),
        paint(true, DIM, &"\u{2591}".repeat(10 - filled))
    )
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

/// Terminal columns `s` occupies once printed: SGR color codes take none,
/// emoji take two, and a variation selector (the invisible marker on
/// `WARNING_EMOJI` that picks its emoji-style glyph) takes none.
/// `chars().count()` would size the scorecard border wrong wherever one lands;
/// this covers the handful of symbols this reporter prints without a full
/// Unicode width table.
fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        width += match c as u32 {
            0x1b => {
                // Skip the escape sequence through its final `m`.
                chars.by_ref().find(|&e| e == 'm');
                0
            }
            0xfe00..=0xfe0f => 0,
            0x2600..=0x26ff | 0x2700..=0x27bf | 0x1f300..=0x1faff if c != '\u{2714}' => 2,
            _ => 1,
        };
    }
    width
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
        results.iter().try_for_each(|r| {
            let mut findings: Vec<Finding> = if self.quiet {
                filter_errors(&r.findings)
            } else {
                r.findings.clone()
            };
            if (self.quiet && findings.is_empty()) || r.score == 100 {
                return Ok(());
            }
            // Rules report in registry order, which jumps around the file; a
            // reader fixing the file wants top to bottom. The sort is stable,
            // so findings on one line keep registry order.
            findings.sort_by_key(|f| f.line);
            if color {
                // Gutter wide enough for this file's largest line number, so
                // the `│` runs in one straight column.
                let gutter = findings
                    .iter()
                    .map(|f| f.line.to_string().len())
                    .max()
                    .unwrap_or(1);
                writeln!(
                    w,
                    "{} {}  {} {}",
                    paint(color, DIM, "\u{25b8}"),
                    paint(color, BOLD, &r.path),
                    score_bar(r.score),
                    paint_score(color, r.score)
                )?;
                findings.iter().try_for_each(|f| {
                    let line = if f.line > 0 {
                        f.line.to_string()
                    } else {
                        "\u{b7}".to_string()
                    };
                    let (emoji, code) = match f.severity {
                        Severity::Error => (ERROR_EMOJI, RED),
                        Severity::Warning => (WARNING_EMOJI, YELLOW),
                    };
                    writeln!(
                        w,
                        "  {} {} {emoji} {}  {}",
                        paint(color, DIM, &format!("{line:>gutter$}")),
                        paint(color, DIM, "\u{2502}"),
                        paint(color, code, &f.message),
                        paint(color, DIM, f.rule),
                    )
                })?;
            } else {
                writeln!(w, "{}  {}", r.path, r.score)?;
                findings.iter().try_for_each(|f| {
                    let location = if f.line > 0 {
                        format!("{}: ", f.line)
                    } else {
                        String::new()
                    };
                    writeln!(w, "  {location}{}: {}: {}", f.severity, f.rule, f.message)
                })?;
            }
            writeln!(w)
        })?;

        // Perfect-score files are not listed above, so the card is the only
        // place that accounts for them.
        let clean_files = results.iter().filter(|r| r.score == 100).count();
        write_scorecard(w, color, summary, clean_files)
    }
}

/// Closes the report. Color mode draws a rounded, dim-bordered card titled in
/// its top edge, with the score bar and band over the file counts. Plain mode
/// is two greppable lines.
fn write_scorecard(
    w: &mut dyn Write,
    color: bool,
    s: &Summary,
    clean_files: usize,
) -> io::Result<()> {
    let band = score_band_label(s.score);
    let checked = plural(s.files, "file", "files");
    if !color {
        let errors = plural(s.files_with_errors, "file with errors", "files with errors");
        let warnings = plural(
            s.files_with_warnings,
            "file with warnings",
            "files with warnings",
        );
        let clean = if clean_files > 0 {
            format!(", {clean_files} clean")
        } else {
            String::new()
        };
        writeln!(w, "Score: {}/100 \u{b7} {band}", s.score)?;
        return writeln!(w, "{checked} checked, {errors}, {warnings}{clean}");
    }

    let healthy = s.files_with_errors == 0 && s.files_with_warnings == 0;
    let headline = format!(
        "{} {} {} {}",
        score_bar(s.score),
        paint_score(color, s.score),
        paint(color, DIM, "\u{b7}"),
        paint(color, score_color(s.score), band)
    );
    let segments = [
        Some(paint(
            color,
            if healthy { GREEN } else { "" },
            &format!("{checked} checked"),
        )),
        (s.files_with_errors > 0).then(|| {
            format!(
                "{ERROR_EMOJI} {}",
                paint(color, RED, &format!("{} with errors", s.files_with_errors))
            )
        }),
        (s.files_with_warnings > 0).then(|| {
            format!(
                "{WARNING_EMOJI} {}",
                paint(
                    color,
                    YELLOW,
                    &format!("{} with warnings", s.files_with_warnings)
                )
            )
        }),
        (clean_files > 0).then(|| paint(color, GREEN, &format!("\u{2714} {clean_files} clean"))),
    ];
    let counts = segments
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ");

    // The title sits in the top edge, so the box needs no title row of its own.
    const TITLE: &str = " Summary ";
    let inner = visible_width(&headline)
        .max(visible_width(&counts))
        .max(TITLE.len())
        + 4;
    let edge = |s: &str| paint(color, DIM, s);
    let row = |content: &str| {
        let pad = " ".repeat(inner - 4 - visible_width(content));
        format!("{}  {content}{pad}  {}", edge("\u{2502}"), edge("\u{2502}"))
    };
    writeln!(
        w,
        "{}",
        edge(&format!(
            "\u{256d}\u{2500}{TITLE}{}\u{256e}",
            "\u{2500}".repeat(inner - 1 - TITLE.len())
        ))
    )?;
    writeln!(w, "{}", row(&headline))?;
    writeln!(w, "{}", row(&counts))?;
    writeln!(
        w,
        "{}",
        edge(&format!("\u{2570}{}\u{256f}", "\u{2500}".repeat(inner)))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Counts;
    use crate::domain::Kind;
    use crate::domain::summarize;
    use crate::domain::test_utils::results;

    fn render_text(results: &[FileResult], quiet: bool, color: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        TextReporter { quiet, color }
            .render(&mut buf, results, &summarize(results))
            .unwrap();
        buf
    }

    #[test]
    fn text_output_shape() {
        // Given
        let input = results();

        // When
        let buf = render_text(&input, false, false);

        // Then
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 8, "{out}");
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
        // The run's score, then the file counts.
        assert_eq!(lines[6], "Score: 73/100 · Worth a look", "{out}");
        assert_eq!(
            lines[7], "2 files checked, 1 file with errors, 1 file with warnings",
            "{out}"
        );
    }

    /// A file with a perfect score is dropped entirely -- there's nothing to
    /// flag -- in both the normal and `--quiet` views.
    #[test]
    fn text_drops_files_with_a_perfect_score() {
        // Given
        let clean = vec![FileResult {
            path: "skills/good/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 100,
            findings: vec![],
        }];

        // When
        let normal = String::from_utf8(render_text(&clean, false, false)).unwrap();
        let quiet = String::from_utf8(render_text(&clean, true, false)).unwrap();

        // Then
        assert!(!normal.contains("skills/good/SKILL.md"), "{normal}");
        assert!(normal.contains("Score: 100/100 · Healthy"), "{normal}");
        assert!(!quiet.contains("skills/good/SKILL.md"), "{quiet}");
    }

    #[test]
    fn text_orders_findings_by_line_within_a_file() {
        // Given
        let finding = |line, rule: &'static str| Finding {
            line,
            severity: Severity::Warning,
            rule,
            message: "m".to_string(),
        };
        let input = vec![FileResult {
            path: "SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 80,
            findings: vec![
                finding(4, "a.late"),
                finding(2, "b.early"),
                finding(2, "c.tie"),
            ],
        }];

        // When
        let out = String::from_utf8(render_text(&input, false, false)).unwrap();

        // Then
        let at = |needle: &str| out.find(needle).unwrap();
        assert!(at("b.early") < at("c.tie"), "{out}");
        assert!(at("c.tie") < at("a.late"), "{out}");
    }

    #[test]
    fn text_summary_counts_clean_files() {
        // Given
        let mut input = results();
        input.push(FileResult {
            path: "skills/good/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 100,
            findings: vec![],
        });

        // When
        let out = String::from_utf8(render_text(&input, false, false)).unwrap();

        // Then
        assert!(out.contains("3 files checked"), "{out}");
        assert!(out.contains(", 1 clean"), "{out}");
    }

    #[test]
    fn color_summary_is_a_titled_rounded_card() {
        // Given
        let mut input = results();
        input.push(FileResult {
            path: "ok/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 100,
            findings: vec![],
        });

        // When
        let out = String::from_utf8(render_text(&input, false, true)).unwrap();

        // Then
        assert!(out.contains("Summary"), "{out}");
        assert!(out.contains("3 files checked"), "{out}");
        assert!(out.contains("1 with errors"), "{out}");
        assert!(out.contains("1 clean"), "{out}");
        assert!(out.contains('╭') && out.contains('╯'), "{out}");
    }

    #[test]
    fn text_quiet_suppresses_warnings() {
        // Given
        let input = results();

        // When
        let buf = render_text(&input, true, false);

        // Then
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains("warning: "));
        assert!(out.contains("1 file with warnings"));
    }

    #[test]
    fn text_singular_plural() {
        // Given
        let input: Vec<FileResult> = vec![];

        // When
        let buf = render_text(&input, false, false);

        // Then
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("Score: 100/100 · Healthy"), "{out}");
        assert!(
            out.contains("0 files checked, 0 files with errors, 0 files with warnings"),
            "{out}"
        );
    }

    #[test]
    fn text_color_adds_symbols_and_sgr_codes() {
        // Given
        let input = results();

        // When
        let buf = render_text(&input, false, true);

        // Then
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
        // Given
        let input: Vec<FileResult> = vec![];

        // When
        let buf = render_text(&input, false, true);

        // Then
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains(GREEN), "{out}");
        assert!(!out.contains(RED), "{out}");
        assert!(!out.contains(YELLOW), "{out}");
    }

    /// A mid-range score bad enough to fall short of yellow still isn't the
    /// worst band: it's painted orange rather than red.
    #[test]
    fn paint_score_orange_band() {
        // Given
        let score = 60;

        // When
        let painted = paint_score(true, score);

        // Then
        assert_eq!(painted, format!("{ORANGE}60{RESET}"));
    }
}
