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

- `src/main.rs` — entry point only; all logic lives in the other modules.
- `src/lib.rs` — the module list. The crate is a library as well as a binary so
  `web/` can read settings such as the default budgets from it rather than
  restating numbers that would drift.
- `src/cli.rs` — flags (a `clap` derive on `Flags`), orchestration, exit codes. `Cli` is built (validated) from
  the merged settings, then executed. `run` takes its writers as arguments so
  tests drive the whole CLI in-process. The four token-budget
  fields on `Flags` are `Option`s so an unset flag can fall through to the
  config file; `resolve` merges flags over the file over the defaults. Run
  behavior (`strict`, `quiet`, `format`, `color`) is flag-only and does not
  go through the config file.
- `src/config.rs` — the `.lintmatter.yaml` loader and its discovery walk. Only
  budgets, `exclude` and `rules` are configurable there; see the module doc
  comment for why run-behavior flags are excluded.
- `src/discover.rs` — `Discoverer` turns paths into targets, pruning dependency
  directories. Each `Target` carries the root it was found under.
- `src/parse.rs` — `Document::parse` splits YAML front matter from the body,
  keeping line numbers. `Document::mapping` is `Some` only when the block is
  usable, which is how rules short-circuit on a malformed one.
- `src/fence.rs` — `FenceTracker`, so fenced code blocks can be skipped.
- `src/tokens.rs` — heuristic token estimator behind the `Counter` trait.
- `src/lint.rs` + `src/lint/` — `linter.rs`'s `Linter` measures token counts,
  then runs every registered rule. `ids.rs` holds the rule ids, `finding.rs`
  the result types, `sink.rs` `FindingSink` and the score `Part`s, `context.rs`
  `FileContext`, `score.rs` the per-file score. `rule.rs` holds the `Rule`
  trait; `rules/` holds one struct per rule id and `registry.rs`'s
  `rules::all()`, the registry.
- `src/report.rs` + `src/report/` — the `Report` trait (`reporter.rs`) with
  `TextReporter` and `JsonReporter`; `summary.rs` totals a run.
- `src/utils.rs` — helpers belonging to no module: `humanize`, `plural`,
  `to_slash`, `clean_path`, `ceil_div`.
- `tests/cli.rs` — end-to-end tests over `testdata/`; unit tests live beside their code.
- `web/` — the `lintmatter-web` crate: an axum server that clones a GitHub repo
  and runs the `lintmatter` binary over it. It listens on `HOST`/`PORT`
  (default `127.0.0.1:3000`); a container host wants `HOST=0.0.0.0`. `lint.rs`'s
  `Budgets` turns the form's token budgets into `--max-*` flags; `main.rs`
  substitutes the defaults into `index.html`'s `{{BUDGET_SETTINGS}}` at render
  time, so the form opens on the linter's own numbers.

## Adding a rule

1. Add its id constant in `src/lint/ids.rs`, then a struct implementing `Rule`
   in the matching `src/lint/rules/` module. Emit through `FindingSink::error`
   or `warn`; the sink knows which rule is running and handles `--strict`.
   Override `applies_to` if the rule judges AGENTS.md too — the default is
   SKILL.md only.
2. Give it a `Part` in `lint/sink.rs`'s `part`, and call `FindingSink::applies`
   at the point in `check` where the rule's precondition holds, ahead of the
   condition that fires it, so the rule lands in the score's denominator
   whether or not it fires. Call it only where the rule really had something
   to judge: a check skipped for this file must not count against it.
   `error`/`warn` call it implicitly too, so a rule that fires is always
   counted even if the check site forgot to call `applies` first.
3. Register it in `rules::all()` (`lint/rules/registry.rs`) **at the position
   it should be reported**. That order is the only place report order is written down: it drives
   `RULES`, `--list-rules`, `--disable` validation, the config file's `rules:`
   mapping, and the order findings appear in.
4. Add a case to the table in `lint/linter.rs`'s test module asserting the exact
   rule ids the fixture produces.
5. Document it in the README's rule table. Nothing else is needed for
   `--disable` or the config file's `rules:` mapping: both validate against
   `RULES`.

## Adding a setting

A setting that belongs in a project's config file needs three edits: an
`Option` field on `cli.rs`'s `Flags` plus its `#[arg]` attribute, a key in
`config.rs`'s `KNOWN_KEYS` and its arm in `parse`, and a line in `resolve` that
picks flag over file over default. Document it in the README's flag table. A
token budget needs two more: a field on `web/src/lint.rs`'s `Budgets` with its
entry in `entries`, and an input in `index.html` whose `id` is that field name.
Run-behavior settings (how a result is reported, not what counts as a
violation) belong on `Flags` alone, flag-only — see `config.rs`'s module doc
comment.

## Conventions

- A module's index is `foo.rs` beside `foo/`, never `foo/mod.rs`
  (`clippy::mod_module_files` enforces it), and holds only `mod` declarations
  and `pub use` re-exports. Code and tests live in the files under `foo/`.
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
