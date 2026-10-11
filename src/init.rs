//! `lintmatter init`: a walkthrough that writes a `.lintmatter.yaml`.
//!
//! Hand-writing the file means guessing key names, default values and rule
//! ids, and every one of those is a hard error when lintmatter next reads it. The
//! wizard asks instead, and builds its questions from the same constants and
//! [`RULES`] the parser validates against, so a generated file cannot name a
//! setting or a rule that does not exist.

use std::io::{BufRead, Write};
use std::path::Path;

use crate::cli::{EXIT_OK, EXIT_USAGE, parse_bool_inline, split_flag};
use crate::config::{
    self, DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS, Settings,
};
use crate::lint::RULES;

/// Runs the wizard and returns the process exit code. The target directory is
/// a parameter rather than read from the process, both so tests can point it
/// at a temporary directory without mutating the shared working directory and
/// so the write target is never ambiguous.
pub fn run(
    args: &[String],
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    dir: &Path,
) -> i32 {
    let mut force = false;
    for arg in args {
        if !arg.starts_with('-') || arg == "-" {
            let _ = writeln!(stderr, "lintmatter: init takes no paths: {arg:?}");
            print_usage(stderr);
            return EXIT_USAGE;
        }
        let (name, inline) = split_flag(arg);
        match name {
            "h" | "help" => {
                print_usage(stderr);
                return EXIT_OK;
            }
            "f" | "force" => force = parse_bool_inline(inline),
            other => {
                let _ = writeln!(
                    stderr,
                    "lintmatter: flag provided but not defined: -{other}"
                );
                print_usage(stderr);
                return EXIT_USAGE;
            }
        }
    }

    // Overwriting a tuned config by accident is worse than a second command,
    // so an existing file stops the run unless it was asked for. When --force
    // does allow it, the file already there is the one rewritten: writing the
    // other spelling instead would leave two configs, only one of which is
    // read.
    let existing = config::FILE_NAMES
        .iter()
        .find(|name| dir.join(name).is_file());
    let target = match existing {
        Some(name) if !force => {
            let _ = writeln!(
                stderr,
                "lintmatter: {name} already exists: pass --force to overwrite it"
            );
            return EXIT_USAGE;
        }
        Some(name) => *name,
        None => config::FILE_NAMES[0],
    };

    // Settings are discovered upward, so a config above this directory already
    // governs it. Say so rather than let the new file look like the only one.
    if existing.is_none()
        && let Some(found) = config::discover(dir)
    {
        let _ = writeln!(
            stdout,
            "note: {} already applies to this directory\n",
            found.display()
        );
    }

    let settings = interview(stdin, stdout);
    let path = dir.join(target);
    if let Err(e) = std::fs::write(&path, render(&settings)) {
        let _ = writeln!(stderr, "lintmatter: cannot write {}: {e}", path.display());
        return EXIT_USAGE;
    }

    let _ = writeln!(stdout, "\nWrote {target}");
    EXIT_OK
}

/// The token budgets, in the order the wizard asks about them and the order
/// the usage text lists them.
const BUDGETS: &[(&str, i64)] = &[
    ("max-agents-tokens", DEFAULT_MAX_AGENTS_TOKENS),
    ("max-skill-tokens", DEFAULT_MAX_SKILL_TOKENS),
    ("max-skill-name-tokens", DEFAULT_MAX_SKILL_NAME_TOKENS),
    (
        "max-skill-description-tokens",
        DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    ),
];

/// Asks every question and returns what the answers add up to. Only settings
/// moved off their default are recorded, so the rendered file says what the
/// project actually chose.
fn interview(stdin: &mut impl BufRead, stdout: &mut impl Write) -> Settings {
    let mut s = Settings::default();
    // End of input ends the walkthrough early: a piped or closed stdin means
    // "take the defaults from here on", which is what makes the command usable
    // in a script instead of spinning on empty reads.
    let _ = walk(stdin, stdout, &mut s);
    s
}

fn walk(stdin: &mut impl BufRead, stdout: &mut impl Write, s: &mut Settings) -> Option<()> {
    let _ = writeln!(
        stdout,
        "Token budgets, in tokens. 0 disables the check; blank keeps the default."
    );

    let slots = [
        &mut s.max_agents_tokens,
        &mut s.max_skill_tokens,
        &mut s.max_skill_name_tokens,
        &mut s.max_skill_description_tokens,
    ];
    for (slot, (key, default)) in slots.into_iter().zip(BUDGETS) {
        let n = ask_budget(stdin, stdout, key, *default)?;
        if n != *default {
            *slot = Some(n);
        }
    }

    let answer = ask(
        stdin,
        stdout,
        "\nPaths to exclude (comma-separated globs, blank for none): ",
    )?;
    s.excludes = split_list(&answer);

    let _ = writeln!(stdout, "\nRules:");
    for (i, rule) in RULES.iter().enumerate() {
        let _ = writeln!(stdout, "  {:2}. {rule}", i + 1);
    }
    loop {
        let answer = ask(
            stdin,
            stdout,
            "Rules to switch off (comma-separated ids or numbers, blank for none): ",
        )?;
        match resolve_rules(&answer) {
            Ok(rules) => {
                s.disabled = rules;
                return Some(());
            }
            Err(msg) => {
                let _ = writeln!(stdout, "  {msg}");
            }
        }
    }
}

/// Asks one question. Returns the trimmed answer, or `None` at end of input.
fn ask(stdin: &mut impl BufRead, stdout: &mut impl Write, prompt: &str) -> Option<String> {
    let _ = write!(stdout, "{prompt}");
    let _ = stdout.flush();
    let mut line = String::new();
    match stdin.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

/// Asks for one budget, re-asking until the answer is one the config parser
/// would accept. The messages match `config::budget`'s, so a value rejected
/// here is rejected in the same words when it comes from a hand-edited file.
fn ask_budget(
    stdin: &mut impl BufRead,
    stdout: &mut impl Write,
    key: &str,
    default: i64,
) -> Option<i64> {
    loop {
        let answer = ask(stdin, stdout, &format!("  {key} [{default}]: "))?;
        if answer.is_empty() {
            return Some(default);
        }
        match answer.parse::<i64>() {
            Ok(n) if n >= 0 => return Some(n),
            Ok(_) => {
                let _ = writeln!(
                    stdout,
                    "  {key} must be zero or more (0 disables the check)"
                );
            }
            Err(_) => {
                let _ = writeln!(
                    stdout,
                    "  invalid value {answer:?} for {key}: not an integer"
                );
            }
        }
    }
}

/// Turns an answer into rule ids, accepting either an id or its number in the
/// printed list. Typos are rejected the way `--disable` rejects them rather
/// than silently switching nothing off, and the result is kept in report order
/// so the rendered mapping reads like `--list-rules`.
fn resolve_rules(answer: &str) -> Result<Vec<String>, String> {
    let mut chosen: Vec<usize> = Vec::new();
    for item in split_list(answer) {
        let index = match item.parse::<usize>() {
            Ok(n) if (1..=RULES.len()).contains(&n) => n - 1,
            Ok(n) => return Err(format!("no rule numbered {n}: pick 1 to {}", RULES.len())),
            Err(_) => RULES.iter().position(|r| *r == item).ok_or_else(|| {
                format!("unknown rule {item:?}: run --list-rules to see them all")
            })?,
        };
        if !chosen.contains(&index) {
            chosen.push(index);
        }
    }
    chosen.sort_unstable();
    Ok(chosen.into_iter().map(|i| RULES[i].to_string()).collect())
}

fn split_list(answer: &str) -> Vec<String> {
    answer
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Renders the config file. Settings left at their default are written
/// commented out with the value they take anyway, so the file documents every
/// key it could carry without pinning one the project never chose.
fn render(s: &Settings) -> String {
    let mut out = String::from(
        "# lintmatter settings. The nearest .lintmatter.yaml at or above the working\n\
         # directory is the one that applies, and flags win over it.\n\n\
         # Token budgets. 0 disables the check.\n",
    );

    let values = [
        s.max_agents_tokens,
        s.max_skill_tokens,
        s.max_skill_name_tokens,
        s.max_skill_description_tokens,
    ];
    for (value, (key, default)) in values.iter().zip(BUDGETS) {
        match value {
            Some(n) => out.push_str(&format!("{key}: {n}\n")),
            None => out.push_str(&format!("# {key}: {default}\n")),
        }
    }

    out.push_str(
        "\n# Paths to skip, matched against both the base name and the path\n\
         # relative to the walk root.\n",
    );
    if s.excludes.is_empty() {
        out.push_str("# exclude:\n#   - testdata\n");
    } else {
        out.push_str("exclude:\n");
        for glob in &s.excludes {
            out.push_str(&format!("  - {glob}\n"));
        }
    }

    out.push_str("\n# Switch a rule off by id (see `lintmatter --list-rules`).\n");
    if s.disabled.is_empty() {
        out.push_str("# rules:\n#   name.dir-mismatch: false\n");
    } else {
        out.push_str("rules:\n");
        for rule in &s.disabled {
            out.push_str(&format!("  {rule}: false\n"));
        }
    }

    out
}

fn print_usage(w: &mut impl Write) {
    let _ = write!(
        w,
        r#"lintmatter init writes a .lintmatter.yaml config file, asking about the token
budgets, the paths to exclude and the rules to switch off.

Usage:
  lintmatter init [--force]

The file is written to the current directory. An existing config there stops
the run unless --force is given. Answering nothing keeps a setting's default,
and end of input takes the defaults for every remaining question, so
`lintmatter init < /dev/null` writes a usable file without prompting.

Flags:
  -f, --force    overwrite an existing config file
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the wizard against a scratch directory, returning the exit code,
    /// the two streams, and whatever config file ended up on disk.
    fn init(
        dir: &tempfile::TempDir,
        args: &[&str],
        answers: &str,
    ) -> (i32, String, String, Option<String>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut stdin = answers.as_bytes();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut stdin, &mut out, &mut err, dir.path());
        let written = config::FILE_NAMES
            .iter()
            .map(|name| dir.path().join(name))
            .find(|p| p.is_file())
            .map(|p| std::fs::read_to_string(p).unwrap());
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            written,
        )
    }

    /// The generated file must be one lintmatter itself accepts, and must mean
    /// exactly what the answers said.
    fn parsed(body: &str) -> Settings {
        config::parse(body, ".lintmatter.yaml").expect("generated config must parse")
    }

    #[test]
    fn blank_answers_write_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (code, stdout, stderr, written) = init(&dir, &[], "\n\n\n\n\n\n");
        assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");

        let body = written.expect("a config file");
        assert_eq!(parsed(&body), Settings::default(), "{body}");
        // Every budget is documented, none of them pinned.
        for (key, default) in BUDGETS {
            assert!(body.contains(&format!("# {key}: {default}\n")), "{body}");
        }
        assert!(body.contains("# rules:"), "{body}");
        assert!(stdout.contains("Wrote .lintmatter.yaml"), "{stdout}");
    }

    #[test]
    fn end_of_input_takes_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (code, stdout, stderr, written) = init(&dir, &[], "");
        assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");
        assert_eq!(
            parsed(&written.expect("a config file")),
            Settings::default()
        );
    }

    #[test]
    fn answers_are_written_and_parse_back() {
        let dir = tempfile::tempdir().unwrap();
        let answers = "3000\n4000\n\n50\ntestdata, tmp\n9, description.length\n";
        let (code, stdout, stderr, written) = init(&dir, &[], answers);
        assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");

        let body = written.expect("a config file");
        assert_eq!(
            parsed(&body),
            Settings {
                max_agents_tokens: Some(3000),
                max_skill_tokens: Some(4000),
                // Left blank, so it stays the default rather than being pinned.
                max_skill_name_tokens: None,
                max_skill_description_tokens: Some(50),
                excludes: vec!["testdata".to_string(), "tmp".to_string()],
                // Chosen by number and by id, reported in rule order.
                disabled: vec![
                    "name.dir-mismatch".to_string(),
                    "description.length".to_string(),
                ],
            },
            "{body}"
        );
    }

    #[test]
    fn a_zero_budget_is_kept_rather_than_read_as_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, _, written) = init(&dir, &[], "0\n\n\n\n\n\n");
        let body = written.expect("a config file");
        assert_eq!(parsed(&body).max_agents_tokens, Some(0), "{body}");
    }

    #[test]
    fn bad_answers_are_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let answers = "-1\nlots\n900\n\n\n\n\nbogus.rule\n99\n2\n";
        let (code, stdout, stderr, written) = init(&dir, &[], answers);
        assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");

        assert!(
            stdout.contains("must be zero or more (0 disables the check)"),
            "{stdout}"
        );
        assert!(stdout.contains(r#"invalid value "lots""#), "{stdout}");
        assert!(stdout.contains(r#"unknown rule "bogus.rule""#), "{stdout}");
        assert!(stdout.contains("no rule numbered 99"), "{stdout}");

        let body = written.expect("a config file");
        assert_eq!(
            parsed(&body),
            Settings {
                max_agents_tokens: Some(900),
                disabled: vec!["frontmatter.not-first".to_string()],
                ..Settings::default()
            },
            "{body}"
        );
    }

    #[test]
    fn an_existing_config_is_not_overwritten_without_force() {
        for name in config::FILE_NAMES {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(name), "max-skill-tokens: 1\n").unwrap();

            let (code, stdout, stderr, written) = init(&dir, &[], "\n\n\n\n\n\n");
            assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
            assert!(
                stderr.contains(&format!("{name} already exists")),
                "{stderr}"
            );
            assert_eq!(written.unwrap(), "max-skill-tokens: 1\n");
        }
    }

    #[test]
    fn force_rewrites_the_file_that_is_already_there() {
        // The .yml spelling is rewritten in place: writing .lintmatter.yaml
        // instead would leave two configs with only the new one being read.
        for name in config::FILE_NAMES {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(name), "max-skill-tokens: 1\n").unwrap();

            let (code, stdout, stderr, _) = init(&dir, &["--force"], "\n\n\n\n\n\n");
            assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");

            let body = std::fs::read_to_string(dir.path().join(name)).unwrap();
            assert_eq!(parsed(&body), Settings::default(), "{body}");
            assert!(stdout.contains(&format!("Wrote {name}")), "{stdout}");
            for other in config::FILE_NAMES.iter().filter(|n| *n != name) {
                assert!(!dir.path().join(other).exists(), "{other} was also written");
            }
        }
    }

    #[test]
    fn a_config_further_up_is_pointed_out() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".lintmatter.yaml"), "").unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();

        let args: Vec<String> = Vec::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut "".as_bytes(), &mut out, &mut err, &nested);
        let stdout = String::from_utf8(out).unwrap();
        assert_eq!(code, EXIT_OK, "stdout={stdout}");
        assert!(stdout.contains("already applies"), "{stdout}");
        assert!(nested.join(".lintmatter.yaml").is_file());
    }

    #[test]
    fn usage_errors() {
        let cases: &[(&str, &[&str], &str)] = &[
            (
                "unknown flag",
                &["--wizard"],
                "flag provided but not defined",
            ),
            ("a path", &["skills/"], "init takes no paths"),
            (
                "a path after a flag",
                &["--force", "."],
                "init takes no paths",
            ),
        ];
        for (name, args, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            let (code, stdout, stderr, written) = init(&dir, args, "\n\n\n\n\n\n");
            assert_eq!(code, EXIT_USAGE, "{name}: stdout={stdout} stderr={stderr}");
            assert!(stderr.contains(want), "{name}: {stderr}");
            assert!(written.is_none(), "{name}: wrote a file anyway");
        }
    }

    #[test]
    fn help_writes_usage_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let (code, _, stderr, written) = init(&dir, &["--help"], "");
        assert_eq!(code, EXIT_OK);
        assert!(stderr.contains("lintmatter init [--force]"), "{stderr}");
        assert!(written.is_none());
    }
}
