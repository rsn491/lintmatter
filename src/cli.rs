//! Wires flags, discovery, linting and reporting together.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{
    self, Config, DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS,
};
use crate::discover;
use crate::lint;
use crate::report;

/// No errors were found; warnings alone still exit OK.
pub const EXIT_OK: i32 = 0;
/// At least one error-severity finding was reported.
pub const EXIT_FINDINGS: i32 = 1;
/// The run could not happen: bad flags or unreadable files.
pub const EXIT_USAGE: i32 = 2;

/// The reported build version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the command line asked for. The four token budgets are optional so an
/// unset flag is distinguishable from one set to its default value and the
/// config file can supply it instead. Run-behavior flags (`strict`, `quiet`,
/// `format`, `color`) are not configurable via the file, so they keep their
/// concrete defaults here.
struct Flags {
    max_agents_tokens: Option<i64>,
    max_skill_tokens: Option<i64>,
    max_skill_name_tokens: Option<i64>,
    max_skill_description_tokens: Option<i64>,
    format: String,
    color: String,
    strict: bool,
    quiet: bool,
    show_version: bool,
    list_rules: bool,
    excludes: Vec<String>,
    disabled: Vec<String>,
    config: Option<String>,
    no_config: bool,
    paths: Vec<String>,
}

impl Default for Flags {
    fn default() -> Self {
        Flags {
            max_agents_tokens: None,
            max_skill_tokens: None,
            max_skill_name_tokens: None,
            max_skill_description_tokens: None,
            format: "text".to_string(),
            color: "auto".to_string(),
            strict: false,
            quiet: false,
            show_version: false,
            list_rules: false,
            excludes: Vec::new(),
            disabled: Vec::new(),
            config: None,
            no_config: false,
            paths: Vec::new(),
        }
    }
}

/// The settings a run actually uses, after the command line and the config
/// file's budgets, excludes and rules have been merged in that order of
/// precedence.
struct Resolved {
    max_agents_tokens: i64,
    max_skill_tokens: i64,
    max_skill_name_tokens: i64,
    max_skill_description_tokens: i64,
    format: String,
    color: String,
    strict: bool,
    quiet: bool,
    excludes: Vec<String>,
    disabled: Vec<String>,
    paths: Vec<String>,
}

/// Merges flags over config-file settings over the built-in defaults. Lists
/// accumulate instead of overriding: `--exclude` and `--disable` add to
/// whatever the file already asked for, since narrowing a run further on the
/// command line is the common case. Run-behavior flags pass straight through:
/// the config file has no say over them.
fn resolve(f: Flags, cfg: config::Settings) -> Resolved {
    let mut excludes = cfg.excludes;
    excludes.extend(f.excludes);
    let mut disabled = cfg.disabled;
    disabled.extend(f.disabled);

    Resolved {
        max_agents_tokens: f
            .max_agents_tokens
            .or(cfg.max_agents_tokens)
            .unwrap_or(DEFAULT_MAX_AGENTS_TOKENS),
        max_skill_tokens: f
            .max_skill_tokens
            .or(cfg.max_skill_tokens)
            .unwrap_or(DEFAULT_MAX_SKILL_TOKENS),
        max_skill_name_tokens: f
            .max_skill_name_tokens
            .or(cfg.max_skill_name_tokens)
            .unwrap_or(DEFAULT_MAX_SKILL_NAME_TOKENS),
        max_skill_description_tokens: f
            .max_skill_description_tokens
            .or(cfg.max_skill_description_tokens)
            .unwrap_or(DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS),
        format: f.format,
        color: f.color,
        strict: f.strict,
        quiet: f.quiet,
        excludes,
        disabled,
        paths: if f.paths.is_empty() {
            vec![".".to_string()]
        } else {
            f.paths
        },
    }
}

/// Finds the config file for this run: the one named by `--config`, or the
/// nearest `.lintmatter.yaml` at or above the working directory. `--no-config`
/// skips the search, and finding nothing is not an error.
fn load_config(f: &Flags, cwd: &Path) -> Result<config::Settings, String> {
    if let Some(path) = &f.config {
        if f.no_config {
            return Err("--config and --no-config cannot be used together".to_string());
        }
        return config::load(Path::new(path));
    }
    if f.no_config {
        return Ok(config::Settings::default());
    }
    match config::discover(cwd) {
        Some(path) => config::load(&path),
        None => Ok(config::Settings::default()),
    }
}

enum ParseOutcome {
    Flags(Flags),
    Help,
    Err(String),
}

/// Splits a token's flag name from any inline `=value`, stripping one or two
/// leading dashes.
fn split_flag(token: &str) -> (&str, Option<&str>) {
    let stripped = token
        .strip_prefix("--")
        .or_else(|| token.strip_prefix('-'))
        .unwrap_or(token);
    match stripped.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (stripped, None),
    }
}

/// Walks the argument list, holding the cursor so a flag needing a value can
/// consume the next argument.
///
/// This used to be two `macro_rules!` blocks inside `parse_args`: one mutated
/// the caller's loop index, the other early-returned from the enclosing
/// function. Both were invisible at the call site. As methods returning
/// `Result`, the control flow is written where it happens.
struct ArgParser<'a> {
    args: &'a [String],
    i: usize,
    /// Budgets given a negative value, collected so every offender is named at
    /// once rather than only the first.
    negative: Vec<String>,
}

impl<'a> ArgParser<'a> {
    fn new(args: &'a [String]) -> Self {
        ArgParser {
            args,
            i: 0,
            negative: Vec::new(),
        }
    }

    /// The value for `--name`: the inline `=value` if given, otherwise the
    /// next argument, which this consumes.
    fn next_value(&mut self, name: &str, inline: Option<&str>) -> Result<String, String> {
        if let Some(v) = inline {
            return Ok(v.to_string());
        }
        self.i += 1;
        self.args
            .get(self.i)
            .cloned()
            .ok_or_else(|| format!("flag needs an argument: --{name}"))
    }

    /// Like [`Self::next_value`], but rejects an empty value.
    fn next_nonempty(&mut self, name: &str, inline: Option<&str>) -> Result<String, String> {
        let v = self.next_value(name, inline)?;
        if v.is_empty() {
            return Err(format!("--{name} value must not be empty"));
        }
        Ok(v)
    }

    /// A token budget. Negatives are recorded rather than rejected here, so
    /// the error can list all of them together once parsing finishes.
    fn budget(&mut self, name: &str, inline: Option<&str>) -> Result<i64, String> {
        let raw = self.next_value(name, inline)?;
        let n: i64 = raw
            .parse()
            .map_err(|_| format!("invalid value {raw:?} for flag --{name}: not an integer"))?;
        if n < 0 {
            self.negative.push(format!("--{name}"));
        }
        Ok(n)
    }
}

fn parse_args(args: &[String]) -> ParseOutcome {
    match parse_flags(args) {
        Ok(outcome) => outcome,
        Err(msg) => ParseOutcome::Err(msg),
    }
}

fn parse_flags(args: &[String]) -> Result<ParseOutcome, String> {
    let mut f = Flags::default();
    let mut p = ArgParser::new(args);

    while p.i < args.len() {
        let arg = &args[p.i];
        if arg == "--" {
            p.i += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }

        let (name, inline) = split_flag(arg);
        // Budgets share a shape, so they are dispatched by table rather than
        // by four near-identical arms.
        let budget = match name {
            "max-agents-tokens" => Some(&mut f.max_agents_tokens),
            "max-skill-tokens" => Some(&mut f.max_skill_tokens),
            "max-skill-name-tokens" => Some(&mut f.max_skill_name_tokens),
            "max-skill-description-tokens" => Some(&mut f.max_skill_description_tokens),
            _ => None,
        };
        if let Some(slot) = budget {
            *slot = Some(p.budget(name, inline)?);
            p.i += 1;
            continue;
        }

        match name {
            "h" | "help" => return Ok(ParseOutcome::Help),
            "version" => f.show_version = true,
            "list-rules" => f.list_rules = true,
            "strict" => f.strict = parse_bool_inline(inline),
            "quiet" => f.quiet = parse_bool_inline(inline),
            "no-config" => f.no_config = parse_bool_inline(inline),
            "format" => f.format = p.next_value(name, inline)?,
            "color" => f.color = p.next_value(name, inline)?,
            "config" => f.config = Some(p.next_nonempty(name, inline)?),
            "exclude" => f.excludes.push(p.next_nonempty(name, inline)?),
            "disable" => f.disabled.push(p.next_nonempty(name, inline)?),
            other => return Err(format!("flag provided but not defined: -{other}")),
        }
        p.i += 1;
    }

    f.paths = args[p.i..].to_vec();

    if !p.negative.is_empty() {
        p.negative.sort();
        return Err(format!(
            "{} must be zero or more (0 disables the check)",
            p.negative.join(", ")
        ));
    }

    Ok(ParseOutcome::Flags(f))
}

fn parse_bool_inline(inline: Option<&str>) -> bool {
    match inline {
        Some(v) => v != "false" && v != "0",
        None => true,
    }
}

/// A validated run: command line and config file merged, values checked.
///
/// Construction decides *what* to do and rejects bad input; `execute` does it.
/// Keeping those apart is what stops `run` from being one procedure that mixes
/// parsing, validation, discovery, linting, reporting and exit codes.
struct Cli {
    settings: Resolved,
}

impl Cli {
    /// Merges the config file under the flags and validates the result.
    ///
    /// Errors are phrased without naming a flag, since a bad value may have
    /// come from either source.
    fn new(f: Flags, cwd: &Path) -> Result<Self, String> {
        // Typos in --disable are caught before the config file is read so the
        // error names the flag the user just typed.
        check_rule_names(&f.disabled)?;
        let file_cfg = load_config(&f, cwd)?;
        let settings = resolve(f, file_cfg);

        if settings.format != "text" && settings.format != "json" {
            return Err(format!(
                "unknown format {:?}: want text or json",
                settings.format
            ));
        }
        if !matches!(settings.color.as_str(), "auto" | "always" | "never") {
            return Err(format!(
                "unknown color {:?}: want auto, always, or never",
                settings.color
            ));
        }
        Ok(Cli { settings })
    }

    /// The linter configuration this run implies.
    fn lint_config(&self) -> Config {
        let s = &self.settings;
        Config {
            max_agents_tokens: s.max_agents_tokens,
            max_skill_tokens: s.max_skill_tokens,
            max_skill_name_tokens: s.max_skill_name_tokens,
            max_skill_description_tokens: s.max_skill_description_tokens,
            disabled: s.disabled.clone(),
            strict: s.strict,
        }
    }

    fn reporter(&self, is_terminal: bool) -> Box<dyn report::Report> {
        if self.settings.format == "json" {
            Box::new(report::JsonReporter {
                quiet: self.settings.quiet,
            })
        } else {
            Box::new(report::TextReporter {
                quiet: self.settings.quiet,
                color: resolve_color(&self.settings.color, is_terminal),
            })
        }
    }

    /// Discovers, lints and reports. Returns the process exit code.
    fn execute(&self, stdout: &mut impl Write, stderr: &mut impl Write, is_terminal: bool) -> i32 {
        let targets = match discover::find(&self.settings.paths, &self.settings.excludes) {
            Ok(t) => t,
            Err(msg) => {
                let _ = writeln!(stderr, "lintmatter: {msg}");
                return EXIT_USAGE;
            }
        };

        let linter = lint::Linter::new(self.lint_config(), None);
        let mut results = Vec::with_capacity(targets.len());
        for t in &targets {
            match linter.file(t) {
                Ok(res) => results.push(res),
                Err(msg) => {
                    let _ = writeln!(stderr, "lintmatter: {msg}");
                    return EXIT_USAGE;
                }
            }
        }

        // Summarized once and handed to the reporter, rather than each reporter
        // recomputing it and the exit code recomputing it again.
        let summary = report::summarize(&results);
        let write_result = self
            .reporter(is_terminal)
            .render(stdout, &results, &summary);
        // A closed pipe is how `lintmatter . | head` ends, not a failure: report
        // whatever the findings warrant and stay quiet, rather than exiting
        // like a bad flag. Every other write failure is still worth reporting.
        if let Err(e) = write_result
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            let _ = writeln!(stderr, "lintmatter: {e}");
            return EXIT_USAGE;
        }

        if summary.files_with_errors > 0 {
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
        ParseOutcome::Help => {
            print_usage(stderr);
            return EXIT_OK;
        }
        ParseOutcome::Err(msg) => {
            let _ = writeln!(stderr, "lintmatter: {msg}");
            print_usage(stderr);
            return EXIT_USAGE;
        }
    };

    // Answered without reading a config file or touching the filesystem.
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
fn resolve_color(choice: &str, is_terminal: bool) -> bool {
    match choice {
        "always" => return true,
        "never" => return false,
        _ => {}
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
        return true;
    }
    is_terminal
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

fn print_usage(w: &mut impl Write) {
    let _ = write!(
        w,
        r#"lintmatter lints agent instruction files: AGENTS.md and SKILL.md.

Usage:
  lintmatter [flags] [path...]

Paths may be files or directories; directories are walked recursively for
AGENTS.md and SKILL.md. With no path given, the current directory is used.

For skills, YAML front matter is validated against the skill spec. For both
kinds, token budgets are enforced on the content, and on a skill's name and
description.

Token budgets, excludes and rules can also live in a config file: the nearest
.lintmatter.yaml (or .lintmatter.yml) at or above the working directory is read
automatically. Flags win over the file.

  max-skill-tokens: 3000
  exclude:
    - testdata
  rules:
    name.dir-mismatch: false

Exit codes: 0 clean (warnings still exit 0), 1 errors found, 2 bad usage.

Flags:
  --max-agents-tokens int              token budget for AGENTS.md content, 0 disables (default {DEFAULT_MAX_AGENTS_TOKENS})
  --max-skill-tokens int                token budget for SKILL.md content, 0 disables (default {DEFAULT_MAX_SKILL_TOKENS})
  --max-skill-name-tokens int           token budget for a skill's name, 0 disables (default {DEFAULT_MAX_SKILL_NAME_TOKENS})
  --max-skill-description-tokens int    token budget for a skill's description, 0 disables (default {DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS})
  --exclude glob                        glob of paths to skip; repeatable
  --disable rule                        rule id to skip; repeatable
  --config path                         read settings from this file instead of searching
  --no-config                           ignore any config file
  --strict                              treat warnings as errors
  --quiet                               report errors only
  --format text|json                    output format (default "text")
  --color auto|always|never             colorize and decorate text output (default "auto")
  --list-rules                          print every rule id and exit
  --version                             print the version and exit
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &tempfile::TempDir, body: &str) -> String {
        let path = dir.path().join(".lintmatter.yaml");
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn config_is_discovered_from_the_working_directory() {
        // The walk starts at the process's working directory, which tests must
        // not mutate, so exercise the discovery and merge steps directly.
        let dir = tempfile::tempdir().unwrap();
        write_config(&dir, "max-skill-tokens: 7\n");
        let nested = dir.path().join("skills/deep");
        std::fs::create_dir_all(&nested).unwrap();

        let flags = Flags::default();
        let cfg = load_config(&flags, &nested).unwrap();
        assert_eq!(cfg.max_skill_tokens, Some(7));

        let resolved = resolve(flags, cfg);
        assert_eq!(resolved.max_skill_tokens, 7);
        assert_eq!(resolved.max_agents_tokens, DEFAULT_MAX_AGENTS_TOKENS);
        assert_eq!(resolved.paths, vec![".".to_string()]);

        let skipped = load_config(
            &Flags {
                no_config: true,
                ..Default::default()
            },
            &nested,
        )
        .unwrap();
        assert_eq!(skipped, config::Settings::default());
    }
}
