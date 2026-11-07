//! End-to-end tests driving `cli::run` in-process over `testdata/`.

// Helper functions here are test code, which clippy only recognises inside `#[test]` fns.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::PathBuf;

use lintmatter::adapters::cli::{EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, run};
use lintmatter::adapters::report;
use lintmatter::domain::lint;

fn fixture(parts: &[&str]) -> String {
    let mut p = PathBuf::from("testdata");
    for part in parts {
        p.push(part);
    }
    p.to_string_lossy().to_string()
}

/// Runs the CLI verbatim, config discovery included.
fn run_raw(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
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
    // Given
    let broken = fixture(&["broken"]);
    let clean = fixture(&["clean"]);

    // When
    let (findings_code, findings_stderr) =
        run_into_failing_writer(std::io::ErrorKind::BrokenPipe, &broken);
    let (clean_code, clean_stderr) =
        run_into_failing_writer(std::io::ErrorKind::BrokenPipe, &clean);
    let (denied_code, denied_stderr) =
        run_into_failing_writer(std::io::ErrorKind::PermissionDenied, &broken);

    // Then
    // Findings still set the exit code, and nothing is said about the pipe.
    assert_eq!(findings_code, EXIT_FINDINGS, "stderr={findings_stderr}");
    assert!(findings_stderr.is_empty(), "stderr={findings_stderr}");
    assert_eq!(clean_code, EXIT_OK, "stderr={clean_stderr}");
    assert!(clean_stderr.is_empty(), "stderr={clean_stderr}");
    // Other write failures are still reported, so this did not widen into
    // swallowing real I/O errors.
    assert_eq!(denied_code, EXIT_USAGE);
    assert!(
        denied_stderr.contains("lintmatter:"),
        "stderr={denied_stderr}"
    );
}

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
    // Given
    let clean = fixture(&["clean"]);

    // When
    let (code, stdout, stderr) = run_args(&[&clean]);

    // Then
    assert_eq!(code, EXIT_OK, "stdout: {stdout} stderr: {stderr}");
    assert!(
        stdout.contains("2 files checked, 0 files with errors, 0 files with warnings"),
        "{stdout}"
    );
}

#[test]
fn broken_tree_exits_one() {
    // Given
    let broken = fixture(&["broken"]);

    // When
    let (code, stdout, _) = run_args(&[&broken]);

    // Then
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
    // Given
    let path = fixture(&["broken", "bad-name", "SKILL.md"]);

    // When
    let (code, stdout, _) = run_args(&[&path]);

    // Then
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
    // Given
    let broken = fixture(&["broken"]);

    // When
    let (code, stdout, _) = run_args(&["--format", "json", &broken]);

    // Then
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
        assert_ne!(file["kind"].as_str().unwrap(), "");
        let score = file["score"].as_u64().unwrap();
        assert!(score <= 100, "{score}");
        total += score;
        for finding in file["findings"].as_array().unwrap() {
            assert_eq!(finding["file"], file["path"]);
            assert_ne!(finding["message"].as_str().unwrap(), "");
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
    // Given
    let skill = fixture(&["clean", "skills", "well-formed", "SKILL.md"]);

    // When
    let (_, stdout, _) = run_args(&["--format", "json", &skill]);

    // Then
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
    // Given
    let clean = fixture(&["clean"]);

    // When
    let (agents_code, agents_stdout, _) = run_args(&[
        "--max-agents-tokens",
        "5",
        "--max-skill-tokens",
        "0",
        &clean,
    ]);
    let (skill_code, skill_stdout, _) = run_args(&[
        "--max-agents-tokens",
        "0",
        "--max-skill-tokens",
        "5",
        &clean,
    ]);
    let (both_code, both_stdout, _) = run_args(&[
        "--max-agents-tokens",
        "0",
        "--max-skill-tokens",
        "0",
        &clean,
    ]);

    // Then
    assert_eq!(agents_code, EXIT_FINDINGS, "{agents_stdout}");
    let flagged = files_with_findings(&agents_stdout);
    assert_eq!(flagged.len(), 1, "{agents_stdout}");
    assert!(flagged[0].ends_with("AGENTS.md"), "{agents_stdout}");

    assert_eq!(skill_code, EXIT_FINDINGS, "{skill_stdout}");
    let flagged = files_with_findings(&skill_stdout);
    assert_eq!(flagged.len(), 1, "{skill_stdout}");
    assert!(flagged[0].ends_with("SKILL.md"), "{skill_stdout}");

    assert_eq!(both_code, EXIT_OK, "{both_stdout}");
}

#[test]
fn name_and_description_budget_flags() {
    // Given
    let skill = fixture(&["clean", "skills", "well-formed", "SKILL.md"]);

    // When
    let (name_code, name_stdout, _) = run_args(&[
        "--max-skill-tokens",
        "0",
        "--max-skill-name-tokens",
        "1",
        &skill,
    ]);
    let (desc_code, desc_stdout, _) = run_args(&[
        "--max-skill-tokens",
        "0",
        "--max-skill-description-tokens",
        "5",
        &skill,
    ]);
    let (default_code, ..) = run_args(&[&skill]);

    // Then
    assert_eq!(name_code, EXIT_FINDINGS);
    assert!(name_stdout.contains(lint::RULE_TOKENS_NAME));
    assert_eq!(desc_code, EXIT_FINDINGS);
    assert!(desc_stdout.contains(lint::RULE_TOKENS_DESCRIPTION));
    assert_eq!(default_code, EXIT_OK);
}

#[test]
fn warnings_alone_exit_zero() {
    // Given
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    // When
    let (code, stdout, _) = run_args(&["--disable", lint::RULE_NAME_FORMAT, &skill]);
    let (strict_code, ..) = run_args(&["--strict", "--disable", lint::RULE_NAME_FORMAT, &skill]);

    // Then
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains(lint::RULE_NAME_DIR_MISMATCH));
    assert_eq!(strict_code, EXIT_FINDINGS);
}

#[test]
fn quiet_suppresses_warnings() {
    // Given
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    // When
    let (_, text_stdout, _) = run_args(&["--quiet", &skill]);
    let (_, json_stdout, _) = run_args(&["--quiet", "--format", "json", &skill]);

    // Then
    assert!(!text_stdout.contains(lint::RULE_NAME_DIR_MISMATCH));
    assert!(text_stdout.contains(lint::RULE_NAME_FORMAT));
    assert!(text_stdout.contains("warning"));

    assert!(!json_stdout.contains(lint::RULE_NAME_DIR_MISMATCH));
    let got: serde_json::Value = serde_json::from_str(&json_stdout).unwrap();
    assert!(got["summary"]["files_with_warnings"].as_u64().unwrap() > 0);
}

#[test]
fn color_flag_controls_decoration() {
    // Given
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);

    // When
    // run_args passes is_terminal=false, so "auto" (the default) stays
    // plain here.
    let (_, auto_stdout, _) = run_args(&[&skill]);
    let (_, always_stdout, _) = run_args(&["--color", "always", &skill]);
    let (_, never_stdout, _) = run_args(&["--color", "never", &skill]);
    let (code, stdout, stderr) = run_args(&["--color", "rainbow", &skill]);

    // Then
    assert!(!auto_stdout.contains('\u{1b}'), "{auto_stdout}");

    assert!(always_stdout.contains('\u{1b}'), "{always_stdout}");
    assert!(
        always_stdout.contains("\u{2716}") || always_stdout.contains("\u{25b2}"),
        "{always_stdout}"
    );

    assert!(!never_stdout.contains('\u{1b}'), "{never_stdout}");

    assert_eq!(code, EXIT_USAGE, "stdout={stdout} stderr={stderr}");
    assert!(stderr.contains("invalid value"), "{stderr}");
}

#[test]
fn exclude_prunes_paths() {
    // Given
    let broken = fixture(&["broken"]);

    // When
    let (code, stdout, _) = run_args(&[
        "--exclude",
        "verbose-description",
        "--exclude",
        "no-frontmatter",
        "--exclude",
        "unterminated",
        "--exclude",
        "bad-name",
        &broken,
    ]);

    // Then
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains("1 file checked"), "{stdout}");
}

#[test]
fn usage_errors() {
    // Given
    let clean = fixture(&["clean"]);
    let cases: &[(&str, Vec<&str>, &str)] = &[
        ("unknown format", vec!["--format", "xml"], "invalid value"),
        (
            "unknown rule",
            vec!["--disable", "no.such.rule"],
            "unknown rule \"no.such.rule\" in --disable: run --list-rules to see them all",
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

        // When
        let (code, stdout, stderr) = run_args(&args);

        // Then
        assert_eq!(code, EXIT_USAGE, "{name}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains(want), "{name}: stderr={stderr}");
        assert!(stdout.is_empty(), "{name}: stdout={stdout}");
    }

    // When
    let (missing_code, missing_stdout, missing_stderr) = run_args(&["nope-does-not-exist"]);
    let (toml_code, toml_stdout, toml_stderr) = run_args(&["Cargo.toml"]);
    let (flag_code, flag_stdout, flag_stderr) = run_args(&["--nope"]);

    // Then
    assert_eq!(
        missing_code, EXIT_USAGE,
        "stdout={missing_stdout} stderr={missing_stderr}"
    );
    assert!(missing_stderr.contains("cannot read"), "{missing_stderr}");

    assert_eq!(
        toml_code, EXIT_USAGE,
        "stdout={toml_stdout} stderr={toml_stderr}"
    );
    assert!(
        toml_stderr.contains("not an AGENTS.md or SKILL.md"),
        "{toml_stderr}"
    );

    assert_eq!(
        flag_code, EXIT_USAGE,
        "stdout={flag_stdout} stderr={flag_stderr}"
    );
    assert!(flag_stderr.contains("unexpected argument"), "{flag_stderr}");
}

#[test]
fn config_file_supplies_settings() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(&dir, "max-agents-tokens: 5\nmax-skill-tokens: 0\n");

    // When
    let (code, stdout, stderr) = run_raw(&["--config", &cfg, &fixture(&["clean"])]);

    // Then
    assert_eq!(code, EXIT_FINDINGS, "stdout={stdout} stderr={stderr}");
    let flagged = files_with_findings(&stdout);
    assert_eq!(flagged.len(), 1, "{stdout}");
    assert!(flagged[0].ends_with("AGENTS.md"), "{stdout}");
}

#[test]
fn config_file_disables_rules() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let skill = fixture(&["broken", "bad-name", "SKILL.md"]);
    let cfg = write_config(
        &dir,
        "rules:\n  name.format: false\n  name.dir-mismatch: false\n",
    );

    // When
    let (code, stdout, _) = run_raw(&["--config", &cfg, &skill]);

    // Then
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(!stdout.contains(lint::RULE_NAME_FORMAT), "{stdout}");
    assert!(!stdout.contains(lint::RULE_NAME_DIR_MISMATCH), "{stdout}");
}

#[test]
fn flags_win_over_the_config_file() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let clean = fixture(&["clean"]);
    let cfg = write_config(&dir, "max-agents-tokens: 5\n");

    // When
    let (code, stdout, stderr) = run_raw(&["--config", &cfg, "--max-agents-tokens", "0", &clean]);

    // Then
    // The flag overrides the file's budget, so the tree comes back clean.
    assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("2 files checked"), "{stdout}");
}

#[test]
fn run_behavior_flags_are_not_configurable_via_the_file() {
    // Given
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

        // When
        let (code, stdout, stderr) = run_raw(&["--config", &cfg, &skill]);

        // Then
        assert_eq!(code, EXIT_USAGE, "{key}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains("unknown setting"), "{key}: {stderr}");
    }

    // Without them in the file, --strict on the command line still works
    // as a run-behavior flag, unaffected by the config file's presence.
    let cfg = write_config(&dir, "rules:\n  name.format: false\n");

    // When
    let (code, ..) = run_raw(&["--config", &cfg, &skill]);
    let (strict_code, ..) = run_raw(&["--config", &cfg, "--strict", &skill]);

    // Then
    assert_eq!(code, EXIT_OK);
    assert_eq!(strict_code, EXIT_FINDINGS);
}

#[test]
fn excludes_and_disables_accumulate() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(
        &dir,
        "exclude:\n  - verbose-description\n  - no-frontmatter\nrules:\n  name.format: false\n",
    );
    let broken = fixture(&["broken"]);

    // When
    let (code, stdout, _) = run_raw(&[
        "--config",
        &cfg,
        "--exclude",
        "unterminated",
        "--exclude",
        "bad-name",
        &broken,
    ]);

    // Then
    assert_eq!(code, EXIT_OK, "{stdout}");
    assert!(stdout.contains("1 file checked"), "{stdout}");
}

#[test]
fn no_config_ignores_the_file() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(&dir, "max-agents-tokens: 5\n");
    let clean = fixture(&["clean"]);

    // When
    let (with_code, ..) = run_raw(&["--config", &cfg, &clean]);
    let (code, stdout, stderr) = run_raw(&["--no-config", &clean]);

    // Then
    assert_eq!(with_code, EXIT_FINDINGS);
    assert_eq!(code, EXIT_OK, "stdout={stdout} stderr={stderr}");
}

#[test]
fn config_usage_errors() {
    // Given
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
            "cannot be used with",
        ),
        (
            "empty config path",
            vec!["--config", ""],
            "a value is required",
        ),
    ];

    for (name, extra, want) in cases {
        let mut args: Vec<&str> = extra.clone();
        args.push(&clean);

        // When
        let (code, stdout, stderr) = run_raw(&args);

        // Then
        assert_eq!(code, EXIT_USAGE, "{name}: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains(want), "{name}: stderr={stderr}");
        assert!(stdout.is_empty(), "{name}: stdout={stdout}");
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_reported_but_does_not_abort_the_run() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "readable instructions\n").unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let locked = sub.join("AGENTS.md");
    std::fs::write(&locked, "locked\n").unwrap();
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();

    // When
    let (code, stdout, stderr) = run_args(&[&dir.path().to_string_lossy()]);

    // Then
    if std::fs::read(&locked).is_ok() {
        return; // running as root: permissions do not apply
    }
    assert!(stderr.contains("cannot read"), "{stderr}");
    assert!(stdout.contains("1 file checked"), "{stdout}");
    assert_eq!(code, EXIT_FINDINGS, "stdout={stdout} stderr={stderr}");
}

#[test]
fn version_and_list_rules() {
    // Given
    let version_args = ["--version"];
    let list_args = ["--list-rules"];

    // When
    let (version_code, version_stdout, _) = run_args(&version_args);
    let (list_code, list_stdout, _) = run_args(&list_args);

    // Then
    assert_eq!(version_code, EXIT_OK);
    assert_eq!(
        version_stdout,
        format!("lintmatter {}\n", env!("CARGO_PKG_VERSION"))
    );

    assert_eq!(list_code, EXIT_OK);
    let listed: Vec<&str> = list_stdout.split_whitespace().collect();
    assert_eq!(listed.len(), lint::RULES.len());
    for rule in lint::RULES.iter() {
        assert!(list_stdout.contains(rule), "{list_stdout}");
    }
}

#[test]
fn help_goes_to_stdout_and_exits_zero() {
    // Given
    let help_flags = ["-h", "--help"];

    for flag in help_flags {
        // When
        let (code, stdout, stderr) = run_args(&[flag]);

        // Then
        assert_eq!(code, EXIT_OK, "{flag}");
        assert!(
            stdout.contains("lintmatter lints agent instruction files"),
            "{flag}: {stdout}"
        );
        assert!(stderr.is_empty(), "{flag}: {stderr}");
    }

    // When
    let (code, stdout, stderr) = run_args(&["--nope"]);

    // Then
    assert_eq!(code, EXIT_USAGE);
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr.contains("unexpected argument") && stderr.contains("Usage:"),
        "{stderr}"
    );
}

#[test]
fn no_paths_defaults_to_current_directory() {
    // Given
    // Avoid mutating the process-wide working directory here since cargo
    // runs tests concurrently; instead confirm the no-args fallback
    // matches passing "." explicitly.
    let no_args: [&str; 0] = [];
    let dot = ["."];

    // When
    let (code_default, stdout_default, _) = run_args(&no_args);
    let (code_dot, stdout_dot, _) = run_args(&dot);

    // Then
    assert_eq!(code_default, code_dot);
    assert_eq!(stdout_default, stdout_dot);
}

#[cfg(unix)]
#[test]
fn a_symlinked_instruction_file_is_not_followed() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "real instructions\n").unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", sub.join("AGENTS.md")).unwrap();

    // When
    let (_, stdout, stderr) = run_args(&[&dir.path().to_string_lossy()]);

    // Then
    assert!(stdout.contains("1 file checked"), "{stdout}");
    assert!(!stdout.contains("root:") && !stderr.contains("root:"));
}

#[test]
fn an_oversized_instruction_file_is_reported_not_loaded() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let big = vec![b'a'; 5 * 1024 * 1024];
    std::fs::write(dir.path().join("AGENTS.md"), big).unwrap();

    // When
    let (code, _, stderr) = run_args(&[&dir.path().to_string_lossy()]);

    // Then
    assert!(stderr.contains("cannot read"), "{stderr}");
    assert_eq!(code, EXIT_FINDINGS);
}

#[cfg(unix)]
#[test]
fn a_reference_through_a_symlink_out_of_the_tree_reveals_nothing() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/", dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("AGENTS.md"),
        "See [a](docs/etc/passwd) and [b](docs/no/such/host/path).\n",
    )
    .unwrap();

    // When
    let (_, stdout, _) = run_args(&[&dir.path().to_string_lossy()]);

    // Then
    assert!(!stdout.contains("does not exist"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn a_directory_name_with_a_backslash_is_skipped_not_misread() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "real\n").unwrap();
    let odd = dir.path().join("a\\b");
    std::fs::create_dir(&odd).unwrap();
    std::fs::write(odd.join("AGENTS.md"), "odd\n").unwrap();

    // When
    let (_, stdout, _) = run_args(&[&dir.path().to_string_lossy()]);

    // Then
    assert!(stdout.contains("1 file checked"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn an_unreadable_subdirectory_does_not_abort_the_run() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "real\n").unwrap();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();

    // When
    let (code, stdout, _) = run_args(&[&dir.path().to_string_lossy()]);
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    // Then
    assert!(stdout.contains("1 file checked"), "{stdout}");
    assert_ne!(code, EXIT_USAGE);
}
