//! The `--help` text.

use std::io::Write;

use crate::model::FIELDS;

/// The column flag descriptions start at.
const FLAG_WIDTH: usize = 38;

pub(super) fn print_usage(w: &mut impl Write) {
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

Exit codes: 0 clean (warnings still exit 0), 1 errors found or a file could
not be read, 2 bad usage.

Flags:
"#
    );
    // The budget flags come from the same table the parsers read, so the
    // help cannot list a budget the parsers do not accept.
    for field in &FIELDS {
        flag(
            w,
            &format!("--{} int", field.key),
            &format!("{}, 0 disables (default {})", field.help, field.default),
        );
    }
    for (name, help) in [
        ("--exclude glob", "glob of paths to skip; repeatable"),
        ("--disable rule", "rule id to skip; repeatable"),
        (
            "--config path",
            "read settings from this file instead of searching",
        ),
        ("--no-config", "ignore any config file"),
        ("--strict", "treat warnings as errors"),
        ("--quiet", "report errors only"),
        ("--format text|json", r#"output format (default "text")"#),
        (
            "--color auto|always|never",
            r#"colorize and decorate text output (default "auto")"#,
        ),
        ("--list-rules", "print every rule id and exit"),
        ("--version", "print the version and exit"),
    ] {
        flag(w, name, help);
    }
}

fn flag(w: &mut impl Write, name: &str, help: &str) {
    let _ = writeln!(w, "  {name:<FLAG_WIDTH$}{help}");
}
