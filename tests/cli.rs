//! End-to-end tests: they drive the whole CLI in-process through `cli::run`,
//! over the fixtures in `testdata/`.
//!
//! They live here rather than inside `src/cli.rs` because that is what they
//! are -- integration tests -- and because keeping them out leaves that module
//! readable.

use std::io::Write;
use std::path::PathBuf;

use lintmatter::cli::{EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, run};
use lintmatter::{lint, report};

fn fixture(parts: &[&str]) -> String {
    let mut p = PathBuf::from("testdata");
    for part in parts {
        p.push(part);
    }
    p.to_string_lossy().to_string()
}

/// Runs the CLI verbatim, config discovery included.
fn run_raw(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(&args, &mut out, &mut err, false);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Fails every write with a fixed `ErrorKind`, standing in for a closed
/// pipe without needing a real one.
struct FailingWriter(std::io::ErrorKind);

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(self.0))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::from(self.0))
    }
}

fn run_into_failing_writer(kind: std::io::ErrorKind, path: &str) -> (i32, String) {
    let args = vec![path.to_string()];
    let mut err = Vec::new();
    let code = run(&args, &mut FailingWriter(kind), &mut err, false);
    (code, String::from_utf8(err).unwrap())
}

#[test]
fn broken_pipe_is_not_a_usage_error() {
    use std::io::ErrorKind;

    // Findings still set the exit code, and nothing is said about the pipe.
    let (code, stderr) = run_into_failing_writer(ErrorKind::BrokenPipe, &fixture(&["broken"]));
    assert_eq!(code, EXIT_FINDINGS, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");

    let (code, stderr) = run_into_failing_writer(ErrorKind::BrokenPipe, &fixture(&["clean"]));
    assert_eq!(code, EXIT_OK, "stderr={stderr}");
    assert!(stderr.is_empty(), "stderr={stderr}");

    // Other write failures are still reported, so this did not widen into
    // swallowing real I/O errors.
    let (code, stderr) =
        run_into_failing_writer(ErrorKind::PermissionDenied, &fixture(&["broken"]));
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("lintmatter:"), "stderr={stderr}");
}

/// Runs the CLI with config discovery off, so these cases keep testing
/// flags and defaults no matter what .lintmatter.yaml happens to sit above
/// the directory the tests run in.
/// The files lintmatter reported a finding under. Every file gets a header
/// line carrying its score now, so a path appearing in the output is no
/// longer proof that a rule fired on it -- only an indented line below the
/// header is.
fn files_with_findings(stdout: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut header = "";
    for line in stdout.split('\n') {
        if line.starts_with("  ") {
            if !header.is_empty() && !out.contains(&header) {
                out.push(header);
            }
        } else if let Some((path, _)) = line.rsplit_once("  ") {
            header = path;
        }
    }
    out
}

fn run_args(args: &[&str]) -> (i32, String, String) {
    let mut with_flag = vec!["--no-config"];
    with_flag.extend_from_slice(args);
    run_raw(&with_flag)
}

fn write_config(dir: &tempfile::TempDir, body: &str) -> String {
    let path = dir.path().join(".lintmatter.yaml");
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn clean_tree_exits_zero() {
    let (code, stdout, stderr) = run_args(&[&fixture(&["clean"])]);
    assert_eq!(code, EXIT_OK, "stdout: {stdout} stderr: {stderr}");
    assert!(
        stdout.contains("2 files checked, 0 files with errors, 0 files with warnings"),
        "{stdout}"
    );
}

#[test]
fn broken_tree_exits_one() {
    let (code, stdout, _) = run_args(&[&fixture(&["broken"])]);
    assert_eq!(code, EXIT_FINDINGS);
    for want in [
        lint::RULE_FRONTMATTER_MISSING,
        lint::RULE_FRONTMATTER_UNTERMINATED,
        lint::RULE_NAME_FORMAT,
        lint::RULE_NAME_DIR_MISMATCH,
        lint::RULE_FRONTMATTER_UNKNOWN_KEY,
        lint::RULE_DESCRIPTION_LENGTH,
        lint::RULE_TOKENS_DESCRIPTION,
    ] {
        assert!(stdout.contains(want), "stdout missing {want}:\n{stdout}");
    }
    assert!(!stdout.contains("node_modules"), "{stdout}");
}

#[test]
fn text_output_format() {
    let path = fixture(&["broken", "bad-name", "SKILL.md"]);
    let (code, stdout, _) = run_args(&[&path]);
    assert_eq!(code, EXIT_FINDINGS);

    let mut header = "";
    let mut name_format = "";
    for line in stdout.split('\n') {
        if line.contains("SKILL.md") && !line.starts_with("  ") {
            header = line;
        }
        if line.contains(lint::RULE_NAME_FORMAT) {
            name_format = line;
        }
    }
    assert!(!header.is_empty(), "{stdout}");
    assert!(!name_format.is_empty(), "{stdout}");
    let trimmed = name_format.strip_prefix("  ").unwrap();
    let parts: Vec<&str> = trimmed.splitn(4, ": ").collect();
    assert_eq!(parts.len(), 4, "{name_format}");
    assert_eq!(parts[0], "2");
    assert_eq!(parts[1], "error");
    assert_eq!(parts[2], lint::RULE_NAME_FORMAT);
}

#[test]
fn json_output() {
    let (code, stdout, _) = run_args(&["--format", "json", &fixture(&["broken"])]);
    assert_eq!(code, EXIT_FINDINGS);
    let got: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(got["version"], report::SCHEMA_VERSION);
    let files = got["files"].as_array().unwrap();
    assert_eq!(got["summary"]["files"], files.len() as u64);
    assert!(got["summary"]["files_with_errors"].as_u64().unwrap() > 0);
    let run_score = got["summary"]["score"].as_u64().unwrap();
    assert!(run_score < 100, "{stdout}");

    let mut paths = Vec::new();
    let mut total = 0;
    for file in files {
        paths.push(file["path"].as_str().unwrap().to_string());
        assert!(!file["kind"].as_str().unwrap().is_empty());
        let score = file["score"].as_u64().unwrap();
        assert!(score <= 100, "{score}");
        total += score;
        for finding in file["findings"].as_array().unwrap() {
            assert_eq!(finding["file"], file["path"]);
            assert!(!finding["message"].as_str().unwrap().is_empty());
        }
    }
    let files = files.len() as u64;
    assert_eq!(run_score, total / files, "{stdout}");

    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn json_reports_token_counts() {
    let (_, stdout, _) = run_args(&[
        "--format",
        "json",
        &fixture(&["clean", "skills", "well-formed", "SKILL.md"]),
    ]);
    let got: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let files = got["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    let tok = &files[0]["tokens"];
    assert!(tok["content"].as_u64().unwrap() > 0);
    assert!(tok["name"].as_u64().unwrap() > 0);
    assert!(tok["description"].as_u64().unwrap() > 0);
}

#[test]
fn content_budget_flags() {
    let clean = fixture(&["clean"]);

    let (code, stdout, _) = run_args(&[
        "--max-agents-tokens",
        "5",
        "--max-skill-tokens",
        "0",
        &clean,
    ]);
    assert_eq!(code, EXIT_FINDINGS, "{stdout}");
    let flagged = files_with_findings(&stdout);
    assert_eq!(flagged.len(), 1, "{stdout}");
    assert!(flagged[0].ends_with("AGENTS.md"), "{stdout}");

    let (code, stdout, _) = run_args(&[
        "--max-agents-tokens",
        "0",
        "--max-skill-tokens",
        "5",
        &clean,
    ]);
    assert_eq!(code, EXIT_FINDINGS, "{stdout}");
    let flagged = files_with_findings(&stdout);
    assert_eq!(flagged.len(), 1, "{stdout}");
    assert!(flagged[0].ends_with("SKILL.md"), "{stdout}");

    let (code, stdout, _) = run_args(&[
        "--max-agents-tokens",
        "0",
        "--max-skill-tokens",
        "0",
        &clean,
    ]);
    assert_eq!(code, EXIT_OK, "{stdout}");
}

#[test]
fn name_and_description_budget_flags() {
    let skill = fixture(&["clean", "skills", "well-formed", "SKILL.md"]);

    let (code, stdout, _) = run_args(&[
        "--max-skill-tokens",
        "0",
        "--max-skill-name-tokens",
        "1",
        &skill,
    ]);
    assert_eq!(code, EXIT_FINDINGS);
    assert!(stdout.contains(lint::RULE_TOKENS_NAME));

    let (code, stdout, _) = run_args(&[
        "--max-skill-tokens",
        "0",
        "--max-skill-description-tokens",
        "5",
        &skill,
    ]);
    assert_eq!(code, EXIT_FINDINGS);
    assert!(stdout.contains(lint::RULE_TOKENS_DESCRIPTION));

    let (code, ..) = run_args(&[&skill]);
    assert_eq!(code, EXIT_OK);
}

#[test]
fn warnings_alone_exit_zero() {
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    let (code, stdout, _) = run_args(&["--disable", lint::RULE_NAME_FORMAT, &skill]);
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains(lint::RULE_NAME_DIR_MISMATCH));

    let (code, ..) = run_args(&["--strict", "--disable", lint::RULE_NAME_FORMAT, &skill]);
    assert_eq!(code, EXIT_FINDINGS);
}

#[test]
fn quiet_suppresses_warnings() {
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    let (_, stdout, _) = run_args(&["--quiet", &skill]);
    assert!(!stdout.contains(lint::RULE_NAME_DIR_MISMATCH));
    assert!(stdout.contains(lint::RULE_NAME_FORMAT));
    assert!(stdout.contains("warning"));

    let (_, stdout, _) = run_args(&["--quiet", "--format", "json", &skill]);
    assert!(!stdout.contains(lint::RULE_NAME_DIR_MISMATCH));
    let got: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(got["summary"]["files_with_warnings"].as_u64().unwrap() > 0);
}

#[test]
fn color_flag_controls_decoration() {
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    // run_args passes is_terminal=false, so "auto" (the default) stays
    // plain here.
    let (_, stdout, _) = run_args(&[&skill]);
    assert!(!stdout.contains('\u{1b}'), "{stdout}");

    let (_, stdout, _) = run_args(&["--color", "always", &skill]);
    assert!(stdout.contains('\u{1b}'), "{stdout}");
    assert!(
        stdout.contains("\u{274c}") || stdout.contains("\u{26a0}"),
        "{stdout}"
    );

    let (_, stdout, _) = run_args(&["--color", "never", &skill]);
    assert!(!stdout.contains('\u{1b}'), "{stdout}");

    let (code, stdout, stderr) = run_args(&["--color", "rainbow", &skill]);
    assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("unknown color"), "{stderr}");
}

#[test]
fn exclude_prunes_paths() {
    let (code, stdout, _) = run_args(&[
        "--exclude",
        "verbose-description",
        "--exclude",
        "no-frontmatter",
        "--exclude",
        "unterminated",
        "--exclude",
        "bad-name",
        &fixture(&["broken"]),
    ]);
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains("1 file checked"), "{stdout}");
}

#[test]
fn usage_errors() {
    let clean = fixture(&["clean"]);
    let cases: &[(&str, Vec<&str>, &str)] = &[
        ("unknown format", vec!["--format", "xml"], "unknown format"),
        (
            "unknown rule",
            vec!["--disable", "no.such.rule"],
            "unknown rule",
        ),
        (
            "negative budget",
            vec!["--max-agents-tokens", "-5"],
            "must be zero or more",
        ),
        (
            "bad exclude glob",
            vec!["--exclude", "["],
            "invalid exclude",
        ),
    ];
    for (name, extra, want) in cases {
        let mut args: Vec<&str> = extra.clone();
        args.push(&clean);
        let (code, stdout, stderr) = run_args(&args);
        assert_eq!(code, EXIT_USAGE, "{name}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains(want), "{name}: stderr={stderr}");
        assert!(stdout.is_empty(), "{name}: stdout={stdout}");
    }

    let (code, stdout, stderr) = run_args(&["nope-does-not-exist"]);
    assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("cannot read"), "{stderr}");

    let (code, stdout, stderr) = run_args(&["Cargo.toml"]);
    assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("not an AGENTS.md or SKILL.md"), "{stderr}");

    let (code, stdout, stderr) = run_args(&["--nope"]);
    assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("flag provided but not defined"), "{stderr}");
}

#[test]
fn config_file_supplies_settings() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(&dir, "max-agents-tokens: 5\nmax-skill-tokens: 0\n");

    let (code, stdout, stderr) = run_raw(&["--config", &cfg, &fixture(&["clean"])]);
    assert_eq!(code, EXIT_FINDINGS, "stdout={stdout} stderr={stderr}");
    let flagged = files_with_findings(&stdout);
    assert_eq!(flagged.len(), 1, "{stdout}");
    assert!(flagged[0].ends_with("AGENTS.md"), "{stdout}");
}

#[test]
fn config_file_disables_rules() {
    let dir = tempfile::tempdir().unwrap();
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);
    let cfg = write_config(
        &dir,
        "rules:\n  name.format: false\n  name.dir-mismatch: false\n",
    );

    let (code, stdout, _) = run_raw(&["--config", &cfg, &skill]);
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(!stdout.contains(lint::RULE_NAME_FORMAT), "{stdout}");
    assert!(!stdout.contains(lint::RULE_NAME_DIR_MISMATCH), "{stdout}");
}

#[test]
fn flags_win_over_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let clean = fixture(&["clean"]);
    let cfg = write_config(&dir, "max-agents-tokens: 5\n");

    // The flag overrides the file's budget, so the tree comes back clean.
    let (code, stdout, stderr) = run_raw(&["--config", &cfg, "--max-agents-tokens", "0", &clean]);
    assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("2 files checked"), "{stdout}");
}

#[test]
fn run_behavior_flags_are_not_configurable_via_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    // strict, quiet, format and color are not valid config keys.
    for key in [
        "strict: true",
        "quiet: true",
        "format: json",
        "color: never",
    ] {
        let cfg = write_config(&dir, &format!("{key}\n"));
        let (code, stdout, stderr) = run_raw(&["--config", &cfg, &skill]);
        assert_eq!(code, EXIT_USAGE, "{key}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains("unknown setting"), "{key}: {stderr}");
    }

    // Without them in the file, --strict on the command line still works
    // as a run-behavior flag, unaffected by the config file's presence.
    let cfg = write_config(&dir, "rules:\n  name.format: false\n");
    let (code, ..) = run_raw(&["--config", &cfg, &skill]);
    assert_eq!(code, EXIT_OK);
    let (code, ..) = run_raw(&["--config", &cfg, "--strict", &skill]);
    assert_eq!(code, EXIT_FINDINGS);
}

#[test]
fn excludes_and_disables_accumulate() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(
        &dir,
        "exclude:\n  - verbose-description\n  - no-frontmatter\nrules:\n  name.format: false\n",
    );

    let (code, stdout, _) = run_raw(&[
        "--config",
        &cfg,
        "--exclude",
        "unterminated",
        "--exclude",
        "bad-name",
        &fixture(&["broken"]),
    ]);
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains("1 file checked"), "{stdout}");
}

#[test]
fn no_config_ignores_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(&dir, "max-agents-tokens: 5\n");
    let clean = fixture(&["clean"]);

    let (code, ..) = run_raw(&["--config", &cfg, &clean]);
    assert_eq!(code, EXIT_FINDINGS);

    let (code, stdout, stderr) = run_raw(&["--no-config", &clean]);
    assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");
}

#[test]
fn config_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let clean = fixture(&["clean"]);
    let bad = write_config(&dir, "max-skill-tokens: -1\n");
    let missing = dir.path().join("absent.yaml").to_string_lossy().to_string();

    let cases: &[(&str, Vec<&str>, &str)] = &[
        (
            "unreadable config",
            vec!["--config", &missing],
            "cannot read config",
        ),
        (
            "invalid config",
            vec!["--config", &bad],
            "must be zero or more",
        ),
        (
            "config and no-config",
            vec!["--config", &bad, "--no-config"],
            "cannot be used together",
        ),
        (
            "empty config path",
            vec!["--config", ""],
            "must not be empty",
        ),
    ];
    for (name, extra, want) in cases {
        let mut args: Vec<&str> = extra.clone();
        args.push(&clean);
        let (code, stdout, stderr) = run_raw(&args);
        assert_eq!(code, EXIT_USAGE, "{name}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains(want), "{name}: stderr={stderr}");
        assert!(stdout.is_empty(), "{name}: stdout={stdout}");
    }
}

/// A file can be discovered and then fail to open: it vanished in between, or
/// it is a dangling symlink, which the walk adds without following.
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_reported_but_does_not_abort_the_run() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "readable instructions\n").unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::os::unix::fs::symlink("/nonexistent-target", sub.join("AGENTS.md")).unwrap();

    let (code, stdout, stderr) = run_args(&[&dir.path().to_string_lossy()]);

    // The bad file is named on stderr...
    assert!(stderr.contains("cannot read"), "{stderr}");
    // ...the good one is still checked, rather than discarded with the run...
    assert!(stdout.contains("1 file checked"), "{stdout}");
    // ...and the run fails, but as findings rather than as bad usage.
    assert_eq!(code, EXIT_FINDINGS, "stdout={stdout} stderr={stderr}");
}

#[test]
fn version_and_list_rules() {
    let (code, stdout, _) = run_args(&["--version"]);
    assert_eq!(code, EXIT_OK);
    assert!(stdout.starts_with("lintmatter "), "{stdout}");

    let (code, stdout, _) = run_args(&["--list-rules"]);
    assert_eq!(code, EXIT_OK);
    let listed: Vec<&str> = stdout.split_whitespace().collect();
    assert_eq!(listed.len(), lint::RULES.len());
    for rule in lint::RULES.iter() {
        assert!(stdout.contains(rule), "{stdout}");
    }
}

#[test]
fn help_goes_to_stdout_and_exits_zero() {
    for flag in ["-h", "--help"] {
        let (code, stdout, stderr) = run_args(&[flag]);
        assert_eq!(code, EXIT_OK, "{flag}");
        assert!(
            stdout.contains("lintmatter lints agent instruction files"),
            "{flag}: {stdout}"
        );
        assert!(stderr.is_empty(), "{flag}: {stderr}");
    }

    // Usage printed because a flag was wrong is diagnostic output, so it
    // stays on stderr alongside the error and leaves stdout clean.
    let (code, stdout, stderr) = run_args(&["--nope"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr.contains("flag provided but not defined")
            && stderr.contains("lintmatter lints agent instruction files"),
        "{stderr}"
    );
}

#[test]
fn no_paths_defaults_to_current_directory() {
    // Avoid mutating the process-wide working directory here since cargo
    // runs tests concurrently; instead confirm the no-args fallback
    // matches passing "." explicitly.
    let (code_default, stdout_default, _) = run_args(&[]);
    let (code_dot, stdout_dot, _) = run_args(&["."]);
    assert_eq!(code_default, code_dot);
    assert_eq!(stdout_default, stdout_dot);
}
