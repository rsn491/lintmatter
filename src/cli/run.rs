//! Wires flags, settings, discovery, linting and reporting together.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Source};
use crate::lint;
use crate::model::{FileResult, Severity};
use crate::report;
use crate::source::discover;

use super::args::{Flags, ParseOutcome, parse_args};
use super::usage::print_usage;
use super::{EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// A validated run: the merged settings plus the flag-only run behavior.
struct Cli {
    settings: config::Resolved,
    format: Format,
    color: ColorChoice,
    strict: bool,
    quiet: bool,
    paths: Vec<String>,
    cwd: PathBuf,
}

impl Cli {
    fn new(f: Flags, cwd: &Path) -> Result<Self, String> {
        // Checked before the config is read so the error names the typed flag.
        check_rule_names(&f.settings.disabled)?;
        let source = match (f.config, f.no_config) {
            (Some(_), true) => {
                return Err("--config and --no-config cannot be used together".to_string());
            }
            (Some(path), false) => Source::Explicit(PathBuf::from(path)),
            (None, true) => Source::Skip,
            (None, false) => Source::Discover,
        };
        let file = config::load_from(&source, cwd)?;
        let settings = config::resolve(f.settings, file);

        let format = match f.format.as_str() {
            "text" => Format::Text,
            "json" => Format::Json,
            other => return Err(format!("unknown format {other:?}: want text or json")),
        };
        let color = match f.color.as_str() {
            "auto" => ColorChoice::Auto,
            "always" => ColorChoice::Always,
            "never" => ColorChoice::Never,
            other => {
                return Err(format!(
                    "unknown color {other:?}: want auto, always, or never"
                ));
            }
        };

        Ok(Cli {
            settings,
            format,
            color,
            strict: f.strict,
            quiet: f.quiet,
            paths: if f.paths.is_empty() {
                vec![".".to_string()]
            } else {
                f.paths
            },
            cwd: cwd.to_path_buf(),
        })
    }

    fn reporter(&self, is_terminal: bool) -> Box<dyn report::Report> {
        match self.format {
            Format::Json => Box::new(report::JsonReporter),
            Format::Text => Box::new(report::TextReporter {
                color: resolve_color(self.color, is_terminal),
            }),
        }
    }

    fn execute(&self, stdout: &mut impl Write, stderr: &mut impl Write, is_terminal: bool) -> i32 {
        let targets = match discover::find(&self.paths, &self.settings.excludes) {
            Ok(t) => t,
            Err(msg) => {
                let _ = writeln!(stderr, "lintmatter: {msg}");
                return EXIT_USAGE;
            }
        };

        let opts = lint::LintOptions {
            budgets: self.settings.budgets,
            disabled: self.settings.disabled.clone(),
        };
        let linter = lint::Linter::new(opts, self.cwd.clone());
        let mut results = Vec::with_capacity(targets.len());
        // A file can vanish between discovery and linting: skip it, but still fail.
        let mut unreadable = 0usize;
        for t in &targets {
            match linter.file(t) {
                Ok(res) => results.push(res),
                Err(msg) => {
                    let _ = writeln!(stderr, "lintmatter: {msg}");
                    unreadable += 1;
                }
            }
        }

        if self.strict {
            promote_warnings(&mut results);
        }
        let summary = report::summarize(&results);
        if self.quiet {
            drop_warnings(&mut results);
        }

        let write_result = self
            .reporter(is_terminal)
            .render(stdout, &results, &summary);
        // A closed pipe is how `lintmatter . | head` ends, not a failure.
        if let Err(e) = write_result
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            let _ = writeln!(stderr, "lintmatter: {e}");
            return EXIT_USAGE;
        }

        if summary.files_with_errors > 0 || unreadable > 0 {
            EXIT_FINDINGS
        } else {
            EXIT_OK
        }
    }
}

/// Executes lintmatter and returns the process exit code. Findings go to stdout;
/// usage and I/O problems go to stderr. `is_terminal` decides whether
/// `--color auto` colorizes output; callers pass whether their real stdout
/// is a terminal rather than this function inspecting the process's actual
/// file descriptors, so tests can pin the "auto" behavior instead of it
/// depending on however the test binary's stdout happens to be attached.
pub fn run(
    args: &[String],
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    is_terminal: bool,
) -> i32 {
    let f = match parse_args(args) {
        ParseOutcome::Flags(f) => f,
        // Requested usage goes to stdout so `--help | less` works.
        ParseOutcome::Help => {
            print_usage(stdout);
            return EXIT_OK;
        }
        ParseOutcome::Err(msg) => {
            let _ = writeln!(stderr, "lintmatter: {msg}");
            print_usage(stderr);
            return EXIT_USAGE;
        }
    };

    if f.show_version {
        let _ = writeln!(stdout, "lintmatter {VERSION}");
        return EXIT_OK;
    }
    if f.list_rules {
        for rule in lint::RULES.iter() {
            let _ = writeln!(stdout, "{rule}");
        }
        return EXIT_OK;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match Cli::new(f, &cwd) {
        Ok(cli) => cli.execute(stdout, stderr, is_terminal),
        Err(msg) => {
            let _ = writeln!(stderr, "lintmatter: {msg}");
            EXIT_USAGE
        }
    }
}

/// Decides whether text output gets colorized and decorated with symbols.
/// `--color=always`/`never` are absolute; `auto` (the default) follows the
/// [`NO_COLOR`](https://no-color.org) and `CLICOLOR_FORCE` conventions and
/// otherwise colors only when the caller's stdout is a terminal.
fn resolve_color(choice: ColorChoice, is_terminal: bool) -> bool {
    match choice {
        ColorChoice::Always => return true,
        ColorChoice::Never => return false,
        ColorChoice::Auto => {}
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
        return true;
    }
    is_terminal
}

/// Applies `--strict`: every warning becomes an error, and so fails the run.
/// The score is untouched, since it rates which rules fired, not how severely.
fn promote_warnings(results: &mut [FileResult]) {
    for f in results.iter_mut().flat_map(|r| r.findings.iter_mut()) {
        f.severity = Severity::Error;
    }
}

/// Applies `--quiet`: drops warnings from what is reported. Done after the
/// summary is taken, so the run still counts the files that had warnings --
/// quiet is a view, not an edit.
fn drop_warnings(results: &mut [FileResult]) {
    for r in results {
        r.findings.retain(|f| f.severity != Severity::Warning);
    }
}

/// Rejects typos in `--disable` rather than silently doing nothing.
fn check_rule_names(rules: &[String]) -> Result<(), String> {
    let known: std::collections::HashSet<&str> = lint::RULES.iter().copied().collect();
    let mut unknown: Vec<&String> = rules
        .iter()
        .filter(|r| !known.contains(r.as_str()))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort();
    let quoted: Vec<String> = unknown.iter().map(|r| format!("{r:?}")).collect();
    Err(format!(
        "unknown rule {} in --disable: run --list-rules to see them all",
        quoted.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Counts, Finding, Kind};

    fn result(severities: &[Severity]) -> FileResult {
        FileResult {
            path: "skills/thing/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts::default(),
            score: 50,
            findings: severities
                .iter()
                .map(|&severity| Finding {
                    line: 2,
                    rule: "name.dir-mismatch",
                    severity,
                    message: "a finding".to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn strict_promotes_warnings_without_touching_the_score() {
        let mut results = vec![result(&[Severity::Warning, Severity::Error])];
        promote_warnings(&mut results);
        assert_eq!(results[0].errors(), 2);
        assert_eq!(results[0].warnings(), 0);
        assert_eq!(results[0].score, 50);
    }

    /// Quiet runs after the summary, so the run still counts the warnings it
    /// no longer shows.
    #[test]
    fn quiet_drops_warnings_after_the_summary() {
        let mut results = vec![result(&[Severity::Warning]), result(&[Severity::Error])];
        let summary = report::summarize(&results);
        drop_warnings(&mut results);
        assert!(results[0].findings.is_empty());
        assert_eq!(results[1].errors(), 1);
        assert_eq!(summary.files_with_warnings, 1);
    }
}
