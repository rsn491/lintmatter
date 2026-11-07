//! The driving adapter and composition root: parses the command line, wires
//! the filesystem adapters into the lint use case, and turns its outcome into
//! a report and an exit code.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::builder::NonEmptyStringValueParser;
use clap::error::ErrorKind;
use clap::{Parser, ValueEnum};

use crate::adapters::config_file::{self, ConfigError};
use crate::adapters::discover::FsDiscoverer;
use crate::adapters::fs::LocalFs;
use crate::adapters::report;
use crate::application::settings::{self, RunSettings, Settings, UnknownRules};
use crate::application::{LintRun, Outcome};
use crate::domain::config::{
    DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS, DEFAULT_MAX_SKILL_NAME_TOKENS,
    DEFAULT_MAX_SKILL_TOKENS,
};
use crate::domain::lint::{self, Linter};

/// No errors were found; warnings alone still exit OK.
pub const EXIT_OK: i32 = 0;
/// At least one error-severity finding was reported.
pub const EXIT_FINDINGS: i32 = 1;
/// The run could not happen: bad flags, a bad config, or a path that cannot be searched.
/// (A file that turns unreadable mid-run is reported and exits `EXIT_FINDINGS`.)
pub const EXIT_USAGE: i32 = 2;

const LONG_ABOUT: &str = "\
lintmatter lints agent instruction files: AGENTS.md and SKILL.md.

Paths may be files or directories; directories are walked recursively for
AGENTS.md and SKILL.md. With no path given, the current directory is used.

For skills, YAML front matter is validated against the skill spec. For both
kinds, token budgets are enforced on the content, and on a skill's name and
description.";

const AFTER_HELP: &str = "\
Token budgets, excludes and rules can also live in a config file: the nearest
.lintmatter.yaml (or .lintmatter.yml) at or above the working directory is read
automatically. Flags win over the file.

  max-skill-tokens: 3000
  exclude:
    - testdata
  rules:
    name.dir-mismatch: false

Exit codes: 0 clean (warnings still exit 0), 1 errors found or a file could
not be read, 2 bad usage.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum Format {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Why a run could not start: a bad flag, or a setting that does not resolve.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0} in --disable: run --list-rules to see them all")]
    UnknownRules(#[from] UnknownRules),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// What the command line asked for. The four token budgets are optional so an
/// unset flag is distinguishable from one set to its default value and the
/// config file can supply it instead. Run-behavior flags (`strict`, `quiet`,
/// `format`, `color`) are not configurable via the file, so they keep their
/// concrete defaults here.
#[derive(Parser, Default)]
#[command(
    name = "lintmatter",
    version,
    about = "lintmatter lints agent instruction files: AGENTS.md and SKILL.md.",
    long_about = LONG_ABOUT,
    after_long_help = AFTER_HELP
)]
struct Flags {
    #[arg(long, value_name = "INT", value_parser = budget, allow_negative_numbers = true,
        help = format!("Token budget for AGENTS.md content, 0 disables [default: {DEFAULT_MAX_AGENTS_TOKENS}]"))]
    max_agents_tokens: Option<i64>,
    #[arg(long, value_name = "INT", value_parser = budget, allow_negative_numbers = true,
        help = format!("Token budget for SKILL.md content, 0 disables [default: {DEFAULT_MAX_SKILL_TOKENS}]"))]
    max_skill_tokens: Option<i64>,
    #[arg(long, value_name = "INT", value_parser = budget, allow_negative_numbers = true,
        help = format!("Token budget for a skill's name, 0 disables [default: {DEFAULT_MAX_SKILL_NAME_TOKENS}]"))]
    max_skill_name_tokens: Option<i64>,
    #[arg(long, value_name = "INT", value_parser = budget, allow_negative_numbers = true,
        help = format!("Token budget for a skill's description, 0 disables [default: {DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS}]"))]
    max_skill_description_tokens: Option<i64>,
    /// Glob of paths to skip; repeatable
    #[arg(long = "exclude", value_name = "GLOB", value_parser = NonEmptyStringValueParser::new())]
    excludes: Vec<String>,
    /// Rule id to skip; repeatable
    #[arg(long = "disable", value_name = "RULE", value_parser = NonEmptyStringValueParser::new())]
    disabled: Vec<String>,
    /// Read settings from this file instead of searching
    #[arg(long, value_name = "FILE", value_parser = NonEmptyStringValueParser::new())]
    config: Option<String>,
    /// Ignore any config file
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
    /// Treat warnings as errors
    #[arg(long)]
    strict: bool,
    /// Report errors only
    #[arg(long)]
    quiet: bool,
    /// Output format
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
    /// Colorize and decorate text output
    #[arg(long, value_enum, default_value = "auto")]
    color: ColorChoice,
    /// Print every rule id and exit
    #[arg(long)]
    list_rules: bool,
    /// Files or directories to lint
    #[arg(value_name = "PATH")]
    paths: Vec<String>,
}

/// Parses a token budget; 0 is how a check is switched off, so only negatives
/// are rejected.
fn budget(raw: &str) -> Result<i64, String> {
    let n: i64 = raw.parse().map_err(|_| "not an integer".to_string())?;
    if n < 0 {
        return Err("must be zero or more (0 disables the check)".to_string());
    }
    Ok(n)
}

/// Finds the config file for this run: the one named by `--config`, or the
/// nearest `.lintmatter.yaml` at or above the working directory. `--no-config`
/// skips the search, and finding nothing is not an error.
#[expect(clippy::print_stderr)]
fn load_config(f: &Flags, cwd: &Path) -> Result<Settings, CliError> {
    if let Some(path) = &f.config {
        return Ok(config_file::load(Path::new(path))?);
    }
    if f.no_config {
        return Ok(Settings::default());
    }
    match config_file::discover(cwd) {
        Some(path) => {
            // A checked-in config can loosen or disable rules, so say which
            // one is in effect rather than applying it silently.
            if !f.quiet {
                eprintln!("lintmatter: using config {}", path.display());
            }
            Ok(config_file::load(&path)?)
        }
        None => Ok(Settings::default()),
    }
}

/// A validated run: the merged lint settings plus the flag-only choices about
/// how to report them.
struct Cli {
    settings: RunSettings,
    format: Format,
    color: ColorChoice,
    quiet: bool,
    /// Resolved once, so config discovery and the linter agree on it.
    cwd: PathBuf,
}

impl Cli {
    fn new(f: Flags, cwd: &Path) -> Result<Self, CliError> {
        // Checked before the config is read so the error names the typed flag.
        settings::check_rule_names(&f.disabled)?;
        let file_cfg = load_config(&f, cwd)?;
        let flags = Settings {
            max_agents_tokens: f.max_agents_tokens,
            max_skill_tokens: f.max_skill_tokens,
            max_skill_name_tokens: f.max_skill_name_tokens,
            max_skill_description_tokens: f.max_skill_description_tokens,
            excludes: f.excludes,
            disabled: f.disabled,
        };
        Ok(Cli {
            settings: settings::resolve(flags, file_cfg, f.strict, f.paths),
            format: f.format,
            color: f.color,
            quiet: f.quiet,
            cwd: cwd.to_path_buf(),
        })
    }

    fn reporter(&self, is_terminal: bool) -> Box<dyn report::Report> {
        if self.format == Format::Json {
            Box::new(report::JsonReporter { quiet: self.quiet })
        } else {
            Box::new(report::TextReporter {
                quiet: self.quiet,
                color: resolve_color(self.color, is_terminal),
                // Only a real terminal has a width to wrap to; a pipe or a
                // file gets one finding per line.
                width: is_terminal
                    .then(terminal_size::terminal_size)
                    .flatten()
                    .map(|(width, _)| usize::from(width.0)),
            })
        }
    }

    fn execute(self, stdout: &mut impl Write, stderr: &mut impl Write, is_terminal: bool) -> i32 {
        let linter = Linter::new(
            self.settings.config.clone(),
            self.cwd.clone(),
            Box::new(LocalFs),
        );
        let run = LintRun::new(FsDiscoverer, LocalFs, linter);
        let outcome: Outcome = match run.execute(&self.settings.paths, &self.settings.excludes) {
            Ok(o) => o,
            Err(msg) => {
                let _ = writeln!(stderr, "lintmatter: {msg}");
                return EXIT_USAGE;
            }
        };
        outcome.unreadable.iter().for_each(|e| {
            let _ = writeln!(stderr, "lintmatter: {e}");
        });

        let write_result =
            self.reporter(is_terminal)
                .render(stdout, &outcome.results, &outcome.summary);
        // A closed pipe is how `lintmatter . | head` ends, not a failure.
        if let Err(e) = write_result
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            let _ = writeln!(stderr, "lintmatter: {e}");
            return EXIT_USAGE;
        }

        if outcome.failed() {
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
    let argv = std::iter::once("lintmatter").chain(args.iter().map(String::as_str));
    let f = match Flags::try_parse_from(argv) {
        Ok(f) => f,
        // Requested usage goes to stdout so `--help | less` works.
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            let _ = write!(stdout, "{}", e.render());
            return EXIT_OK;
        }
        Err(e) => {
            let _ = write!(stderr, "{}", e.render());
            return EXIT_USAGE;
        }
    };

    if f.list_rules {
        lint::RULES.iter().for_each(|rule| {
            let _ = writeln!(stdout, "{rule}");
        });
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
        // Given
        // The walk starts at the process's working directory, which tests must
        // not mutate, so exercise the discovery and merge steps directly.
        let dir = tempfile::tempdir().unwrap();
        write_config(&dir, "max-skill-tokens: 7\n");
        let nested = dir.path().join("skills/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let flags = Flags::default();
        let no_config_flags = Flags {
            no_config: true,
            ..Default::default()
        };

        // When
        let cfg = load_config(&flags, &nested).unwrap();
        let cli = Cli::new(flags, &nested).unwrap();
        let skipped = load_config(&no_config_flags, &nested).unwrap();

        // Then
        assert_eq!(cfg.max_skill_tokens, Some(7));
        assert_eq!(cli.settings.config.max_skill_tokens, 7);
        assert_eq!(
            cli.settings.config.max_agents_tokens,
            DEFAULT_MAX_AGENTS_TOKENS
        );
        assert_eq!(cli.settings.paths, vec![".".to_string()]);
        assert_eq!(skipped, Settings::default());
    }
}
