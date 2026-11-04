# Working in lintmatter

`lintmatter` lints agent instruction files. It is a Rust CLI; the YAML front
matter is parsed with `saphyr` and flags with `clap`; everything else is
hand-rolled.

## Checks before pushing

```sh
cargo fmt --check       # must report nothing
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Layout

Ports and adapters: dependencies point inward only, `adapters` → `application`
→ `domain`. `tests/architecture.rs` parses each file and enforces this (test
code is exempt): `domain` names only itself and `utils`, `utils` nothing,
`application` adds `domain`; none touch `std::fs`/`env`/`process`/`net`/`thread`,
and each has an allowlist of `[dependencies]` crates.

- `src/main.rs` calls `adapters::cli::run`. `src/lib.rs` lists the layers; the
  crate is also a library so `web/` reads the default budgets
  (`domain::config`) instead of restating them. `src/utils.rs` holds pure
  helpers (`humanize`, `plural`, `to_slash`, `clean_path`, `ceil_div`).
- `src/domain/` — the pure core, no I/O. The model: `kind.rs`, `target.rs`,
  `finding.rs`, `summary.rs`, `config.rs` (`Config`, `DEFAULT_MAX_*`).
  `document.rs` (`Document::parse`; `mapping` is `Some` only when the front
  matter is usable), `fence.rs`, `tokens.rs` (`Counter`). `lint/` is the rules
  engine: `Linter` is handed its cwd and a `PathProbe` (`domain/ports.rs`), plus
  `ids`, `sink`, `score`, `rule` (the `Rule` trait) and `rules/` (one struct
  per rule id; `registry.rs` holds `rules::all()`).
- `src/application/` — use cases. `ports.rs` (`SourceReader`, `TargetFinder`,
  and `PathProbe` re-exported);
  `settings.rs` (`Settings`, `resolve`: flag over file over default,
  `check_rule_names`); `lint_run.rs` (`LintRun` → `Outcome`).
- `src/adapters/` — `cli.rs` (clap `Flags`, exit codes, composition root;
  `run` takes its writers so tests drive it in-process), `config_file.rs`
  (`.lintmatter.yaml` loader and discovery), `discover.rs` (`FsDiscoverer`),
  `fs.rs` (`LocalFs`), `report/` (`Report`, `TextReporter`, `JsonReporter`).
  The JSON shape lives only in `report/json.rs`; domain types carry no `serde`.
- Tests: `tests/cli.rs` runs end to end over `testdata/`; unit tests sit beside
  their code, with shared fixtures in a module's `test_utils.rs`.
- `web/` — the `lintmatter-web` axum server: clones a GitHub repo and runs the
  binary over it. Listens on `HOST`/`PORT` (default `127.0.0.1:3000`; a
  container wants `HOST=0.0.0.0`).
  `lint.rs`'s `Budgets` maps the form to `--max-*` flags; `main.rs` fills
  `index.html`'s `{{BUDGET_SETTINGS}}` with the defaults.

## Adding a rule

1. Add its id constant in `src/domain/lint/ids.rs`, then a struct implementing
   `Rule` in the matching `src/domain/lint/rules/` module. No I/O in a rule;
   go through `FileContext` (e.g. `ctx.exists`). Emit through
   `FindingSink::error` or `warn`; the sink knows which rule is running and handles `--strict`.
   Override `applies_to` if the rule judges AGENTS.md too — the default is
   SKILL.md only.
2. Give it a `Part` in `domain/lint/sink.rs`'s `part`, and call `FindingSink::applies`
   at the point in `check` where the rule's precondition holds, ahead of the
   condition that fires it, so the rule lands in the score's denominator
   whether or not it fires. Call it only where the rule really had something
   to judge: a check skipped for this file must not count against it.
   `error`/`warn` call it implicitly too, so a rule that fires is always
   counted even if the check site forgot to call `applies` first.
3. Register it in `rules::all()` (`domain/lint/rules/registry.rs`) **at the position
   it should be reported**. That order is the only place report order is written down: it drives
   `RULES`, `--list-rules`, `--disable` validation, the config file's `rules:`
   mapping, and the order findings appear in.
4. Add a case to the table in `domain/lint/linter.rs`'s test module asserting the exact
   rule ids the fixture produces.
5. Document it in the README's rule table. Nothing else is needed for
   `--disable` or the config file's `rules:` mapping: both validate against
   `RULES`.

## Adding a setting

A config-file setting needs: an `Option` field on `Settings` and its line in
`resolve` (`application/settings.rs`), plus a `Config` field if the linter reads
it; a `Flags` field with `#[arg]`, mapped in `Cli::new` (`adapters/cli.rs`); a
`KNOWN_KEYS` entry and `parse` arm (`adapters/config_file.rs`); a row in the
README's flag table. A token budget also needs a field and `entries` line on
`web/src/lint.rs`'s `Budgets` and an `index.html` input with that `id`.
Run-behavior settings (how results are reported, not what is a violation) go on
`Flags` alone; see `config_file.rs`'s module doc comment.

## Conventions

- Modules use `foo.rs` + `foo/`, never `mod.rs` (enforced by
  `clippy::mod_module_files`); `foo.rs` only declares and re-exports.
- Prefer errors for anything that breaks a file's contract with the runtime, and
  warnings for stylistic mismatches. Warnings alone exit 0.
- Findings come out in registry order because the rules run in that order, so
  output never depends on the order checks happen to execute in. Nothing sorts
  them afterwards; keep `rules::all()` in report order instead.
- A file's score rates only the checks that ran on it. A part with nothing to
  judge drops out of the mean rather than scoring a free 100.
- Comments explain why a check exists, not what the code does.
- Fallible functions return a `thiserror` error type owned by their module,
  never `String`; the message lives in the `#[error]` attribute.
- Requested usage goes to stdout; diagnostics (including usage after a flag
  error) go to stderr.
- An unreadable file is reported and skipped, not fatal; discovery errors still
  exit 2.
