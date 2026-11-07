//! The human-readable report.

use std::io::{self, Write};

use crate::domain::{FileResult, Finding, Severity};
use crate::utils::{escape_control, plural};

use super::reporter::{Report, filter_errors};
use crate::domain::Summary;

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const ORANGE: &str = "\x1b[38;5;208m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

// Single-column symbols rather than emoji: terminals disagree on whether an
// emoji (and `⚠️`'s variation selector) takes one column or two, which
// misaligned the card and ate the space after the symbol.
const ERROR_SYMBOL: &str = "\u{2716}"; // ✖
const WARNING_SYMBOL: &str = "\u{25b2}"; // ▲

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

/// Terminal columns `s` occupies once printed: SGR color codes take none and
/// emoji take two. `chars().count()` would size the scorecard border wrong
/// wherever one lands; this covers the symbols this reporter prints without a
/// full Unicode width table.
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
            0x1f300..=0x1faff => 2,
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
    /// Terminal width in columns, when known. Only the decorated view wraps
    /// against it; `None` (a pipe, a file) leaves every finding on one line.
    pub width: Option<usize>,
}

/// Narrowest message column worth wrapping into; below it the terminal's own
/// wrapping is no worse than ours.
const MIN_WRAP_WIDTH: usize = 24;

/// A finding's message broken into lines no wider than a budget.
struct Wrapped {
    lines: Vec<String>,
    /// Whether the rule id fits after the last line. When it does not, it
    /// gets a line of its own rather than being cut or pushed past the edge.
    rule_inline: bool,
}

/// Breaks `message` on spaces into lines of at most `budget` characters. A
/// single word wider than the budget keeps a line to itself, since cutting a
/// path or identifier in half helps no one. Widths are counted in chars,
/// which is exact for the ASCII these messages are made of.
fn wrap_finding(message: &str, rule: &str, budget: usize) -> Wrapped {
    let mut lines: Vec<String> = Vec::new();
    message.split(' ').for_each(|word| match lines.last_mut() {
        Some(line) if line.chars().count() + 1 + word.chars().count() <= budget => {
            line.push(' ');
            line.push_str(word);
        }
        _ => lines.push(word.to_string()),
    });
    let last = lines.last().map_or(0, |l| l.chars().count());
    Wrapped {
        rule_inline: last + 2 + rule.chars().count() <= budget,
        lines,
    }
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
                    "{} {}  {}",
                    paint(color, DIM, "\u{25b8}"),
                    paint(color, BOLD, &escape_control(&r.path)),
                    paint_score(color, r.score)
                )?;
                findings.iter().try_for_each(|f| {
                    let line = if f.line > 0 {
                        f.line.to_string()
                    } else {
                        "\u{b7}".to_string()
                    };
                    let (symbol, code) = match f.severity {
                        Severity::Error => (ERROR_SYMBOL, RED),
                        Severity::Warning => (WARNING_SYMBOL, YELLOW),
                    };
                    // Columns taken before the message: indent, gutter, divider
                    // and symbol, each with its spacing.
                    let budget = self
                        .width
                        .map_or(usize::MAX, |cols| cols.saturating_sub(gutter + 7))
                        .max(MIN_WRAP_WIDTH);
                    let wrapped = wrap_finding(&f.message, f.rule, budget);
                    let dim_gutter = |text: &str| {
                        format!(
                            "  {} {}",
                            paint(color, DIM, &format!("{text:>gutter$}")),
                            paint(color, DIM, "\u{2502}")
                        )
                    };
                    wrapped.lines.iter().enumerate().try_for_each(|(i, text)| {
                        // The symbol and line number mark the finding once;
                        // continuation lines hang under the message.
                        let (gutter_text, marker) = if i == 0 {
                            (line.as_str(), paint(color, code, symbol))
                        } else {
                            ("", " ".to_string())
                        };
                        let rule = if wrapped.rule_inline && i + 1 == wrapped.lines.len() {
                            format!("  {}", paint(color, DIM, f.rule))
                        } else {
                            String::new()
                        };
                        writeln!(
                            w,
                            "{} {marker} {}{rule}",
                            dim_gutter(gutter_text),
                            paint(color, code, text)
                        )
                    })?;
                    if wrapped.rule_inline {
                        return Ok(());
                    }
                    writeln!(w, "{}   {}", dim_gutter(""), paint(color, DIM, f.rule))
                })?;
            } else {
                writeln!(w, "{}  {}", escape_control(&r.path), r.score)?;
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
        write_scorecard(w, color, summary, clean_files, self.width)
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
    width: Option<usize>,
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
                "{} {}",
                paint(color, RED, ERROR_SYMBOL),
                paint(color, RED, &format!("{} with errors", s.files_with_errors))
            )
        }),
        (s.files_with_warnings > 0).then(|| {
            format!(
                "{} {}",
                paint(color, YELLOW, WARNING_SYMBOL),
                paint(
                    color,
                    YELLOW,
                    &format!("{} with warnings", s.files_with_warnings)
                )
            )
        }),
        (clean_files > 0).then(|| paint(color, GREEN, &format!("\u{2714} {clean_files} clean"))),
    ];
    let segments: Vec<String> = segments.into_iter().flatten().collect();
    // On a terminal too narrow for one counts line (the box adds six columns
    // of border and padding), stack the counts a row each instead of letting
    // the terminal break the box.
    let joined = segments.join("  ");
    let counts: Vec<String> = if width.is_some_and(|w| visible_width(&joined) + 6 > w) {
        segments
    } else {
        vec![joined]
    };

    // The title sits in the top edge, so the box needs no title row of its own.
    const TITLE: &str = " Summary ";
    let inner = visible_width(&headline)
        .max(counts.iter().map(|c| visible_width(c)).max().unwrap_or(0))
        .max(TITLE.len())
        + 4;
    // Tinted by the run's band rather than dimmed: a dim border all but
    // vanishes on many themes.
    let edge = |text: &str| paint(color, score_color(s.score), text);
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
    counts.iter().try_for_each(|c| writeln!(w, "{}", row(c)))?;
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

    /// Drops SGR color codes so assertions can see the text a terminal shows.
    fn strip_sgr(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                chars.by_ref().find(|&e| e == 'm');
            } else {
                out.push(c);
            }
        }
        out
    }

    fn render_text(results: &[FileResult], quiet: bool, color: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        TextReporter {
            quiet,
            color,
            width: None,
        }
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
    fn wrap_finding_breaks_on_spaces_and_keeps_the_rule_inline_when_it_fits() {
        // Given
        let message = "one two three four five";

        // When
        let wrapped = wrap_finding(message, "a.b", 12);

        // Then
        assert_eq!(wrapped.lines, ["one two", "three four", "five"]);
        assert!(wrapped.rule_inline);
    }

    #[test]
    fn wrap_finding_moves_the_rule_to_its_own_line_when_it_does_not_fit() {
        // Given
        let message = "one two three";

        // When
        let wrapped = wrap_finding(message, "frontmatter.missing", 13);

        // Then
        assert_eq!(wrapped.lines, ["one two three"]);
        assert!(!wrapped.rule_inline);
    }

    #[test]
    fn wrap_finding_never_splits_a_word_wider_than_the_budget() {
        // Given
        let message = "see skills/a-very-long-directory-name/SKILL.md now";

        // When
        let wrapped = wrap_finding(message, "x", 10);

        // Then
        assert!(
            wrapped
                .lines
                .contains(&"skills/a-very-long-directory-name/SKILL.md".to_string()),
            "{:?}",
            wrapped.lines
        );
    }

    #[test]
    fn color_output_wraps_to_the_terminal_width_with_a_hanging_indent() {
        // Given
        let long = "name \"Bad_Name\" must be lowercase letters, digits and single hyphens (for example my-skill)";
        let input = vec![FileResult {
            path: "SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 80,
            findings: vec![Finding {
                line: 12,
                severity: Severity::Error,
                rule: "name.format",
                message: long.to_string(),
            }],
        }];
        let mut buf = Vec::new();
        TextReporter {
            quiet: false,
            color: true,
            width: Some(50),
        }
        .render(&mut buf, &input, &summarize(&input))
        .unwrap();

        // When
        let out = String::from_utf8(buf).unwrap();
        let finding_lines: Vec<&str> = out
            .lines()
            .skip_while(|l| !l.contains("SKILL.md"))
            .skip(1)
            .take_while(|l| !l.is_empty())
            .collect();

        // Then
        assert!(finding_lines.len() > 1, "{out}");
        assert!(
            finding_lines.iter().all(|l| visible_width(l) <= 50),
            "{out}"
        );
        // The line number and symbol appear once; the rest hang beneath.
        assert_eq!(out.matches("12").count(), 1, "{out}");
        assert!(finding_lines[0].contains(ERROR_SYMBOL), "{out}");
        assert!(
            finding_lines[1..].iter().all(|l| !l.contains(ERROR_SYMBOL)),
            "{out}"
        );
        assert!(out.contains("name.format"), "{out}");
    }

    #[test]
    fn color_summary_stacks_its_counts_on_a_narrow_terminal() {
        // Given
        let input = results();
        let render = |width| {
            let mut buf = Vec::new();
            TextReporter {
                quiet: false,
                color: true,
                width,
            }
            .render(&mut buf, &input, &summarize(&input))
            .unwrap();
            String::from_utf8(buf).unwrap()
        };

        // When
        let wide = render(Some(120));
        let narrow = render(Some(40));

        // Then
        let (wide, narrow) = (strip_sgr(&wide), strip_sgr(&narrow));
        let card_rows = |out: &str| out.lines().filter(|l| l.starts_with('│')).count();
        assert_eq!(card_rows(&wide), 2, "{wide}");
        assert!(card_rows(&narrow) > 2, "{narrow}");
        assert!(
            narrow
                .lines()
                .filter(|l| l.starts_with(['╭', '│', '╰']))
                .all(|l| visible_width(l) <= 40),
            "{narrow}"
        );
    }

    #[test]
    fn plain_output_never_wraps() {
        // Given
        let input = results();
        let mut buf = Vec::new();
        TextReporter {
            quiet: false,
            color: false,
            width: Some(20),
        }
        .render(&mut buf, &input, &summarize(&input))
        .unwrap();

        // When
        let out = String::from_utf8(buf).unwrap();

        // Then
        assert_eq!(
            out,
            String::from_utf8(render_text(&input, false, false)).unwrap()
        );
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
        assert!(out.contains(ERROR_SYMBOL), "{out}");
        assert!(out.contains(WARNING_SYMBOL), "{out}");
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
