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

The core crate is laid out as ports and adapters (hexagonal architecture).
Dependencies point inward only: `adapters` → `application` → `domain`.
`tests/architecture.rs` fails the build if `domain` names `application`,
`adapters`, `std::fs`, `std::env`, `clap` or `glob`, or if `application` names
`adapters` or any of those. Test modules and `test_utils.rs` are exempt.

- `src/main.rs` — entry point only; it calls `adapters::cli::run`.
- `src/lib.rs` — the layer list. The crate is a library as well as a binary so
  `web/` can read settings such as the default budgets (`domain::config`)
  rather than restating numbers that would drift.
- `src/utils.rs` — pure helpers that belong to no layer: `humanize`, `plural`,
  `to_slash`, `clean_path`, `ceil_div`.

`src/domain/` is the pure core. It does no I/O.
- `kind.rs` (`Kind`, `kind_for`), `target.rs` (`Target`, which carries the root
  it was found under), `finding.rs` (`Finding`, `Severity`, `Counts`,
  `FileResult`), `summary.rs` (`Summary`, `summarize`).
- `config.rs` — `Config`, the settings the linter applies, and the
  `DEFAULT_MAX_*` budgets.
- `document.rs` — `Document::parse` splits YAML front matter from the body and
  keeps line numbers. `Document::mapping` is `Some` only when the block is
  usable, which is how rules short-circuit on a malformed one.
- `fence.rs` — `FenceTracker`, so fenced code blocks can be skipped.
- `tokens.rs` — the heuristic token estimator behind the `Counter` trait.
- `lint.rs` + `lint/` — the rules engine. `Linter` (`linter.rs`) takes a
  document's bytes, measures its token counts, then runs every registered rule.
  It is given its working directory and a `PathProbe` (`context.rs`), so it
  never reads a file or asks the process anything itself. Also here: `ids.rs`,
  `context.rs`, `sink.rs`, `score.rs`, `rule.rs` (the `Rule` trait) and
  `rules/` (one struct per rule id; `registry.rs` holds `rules::all()`).

`src/application/` holds the use cases. They reach the outside world only
through ports.
- `ports.rs` — `SourceReader` and `TargetFinder`, the driven ports.
- `settings.rs` — `Settings`, the shape both the flags and the config file
  supply. `resolve` merges flags over the file over the defaults into
  `RunSettings`. `check_rule_names` validates `--disable`.
- `lint_run.rs` — `LintRun`, which finds targets, reads and lints each one, and
  summarizes. An unreadable file lands in `Outcome::unreadable` and is skipped.
  `Outcome::failed` decides pass or fail.

`src/adapters/` touches the real world.
- `cli.rs` — the driving adapter and composition root. It holds the flags (a
  `clap` derive on `Flags`) and the exit codes. It wires `FsDiscoverer` and
  `LocalFs` into `LintRun` and picks a reporter. `run` takes its writers as
  arguments so tests drive the whole CLI in-process. The four token-budget
  fields on `Flags` are `Option`s so an unset flag can fall through to the
  config file. Run behavior (`strict`, `quiet`, `format`, `color`) is flag-only
  and does not go through the config file.
- `config_file.rs` — the `.lintmatter.yaml` loader and its discovery walk. Only
  budgets, `exclude` and `rules` are configurable there; the module doc comment
  explains why run-behavior flags are excluded.
- `discover.rs` — `FsDiscoverer` (the `TargetFinder`) turns paths into targets
  and prunes dependency directories.
- `fs.rs` — `LocalFs`, the `SourceReader` and `PathProbe` over `std::fs`.
- `report.rs` + `report/` — the `Report` trait with `TextReporter` and
  `JsonReporter`.

Tests:
- `tests/cli.rs` — end-to-end tests over `testdata/`.
- `tests/architecture.rs` — the dependency rule.
- Unit tests live beside their code. A module's `test_utils.rs` holds fixtures
  shared by its unit tests.
- `web/` — the `lintmatter-web` crate: an axum server that clones a GitHub repo
  and runs the `lintmatter` binary over it. It listens on `HOST`/`PORT`
  (default `127.0.0.1:3000`); a container host wants `HOST=0.0.0.0`. `lint.rs`'s
  `Budgets` turns the form's token budgets into `--max-*` flags; `main.rs`
  substitutes the defaults into `index.html`'s `{{BUDGET_SETTINGS}}` at render
  time, so the form opens on the linter's own numbers.

## Adding a rule

1. Add its id constant in `src/domain/lint/ids.rs`, then a struct implementing
   `Rule` in the matching `src/domain/lint/rules/` module. A rule may not do
   I/O itself; whatever it needs from outside comes through `FileContext`
   (e.g. `ctx.exists`). Emit through `FindingSink::error`
   or `warn`; the sink knows which rule is running and handles `--strict`.
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

A setting that belongs in a project's config file needs these edits:

- `application/settings.rs`: an `Option` field on `Settings`, plus a line in
  `resolve` that picks flag over file over default. If the linter reads the
  setting, it also needs a field on `domain/config.rs`'s `Config`.
- `adapters/cli.rs`: a field on `Flags` with its `#[arg]` attribute, mapped
  into `Settings` in `Cli::new`.
- `adapters/config_file.rs`: a key in `KNOWN_KEYS` and its arm in `parse`.

Document it in the README's flag table. A token budget needs two more edits: a
field on `web/src/lint.rs`'s `Budgets` with its entry in `entries`, and an input
in `index.html` whose `id` is that field name. Run-behavior settings (how a
result is reported, not what counts as a violation) belong on `Flags` alone and
are flag-only; see `config_file.rs`'s module doc comment.

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
