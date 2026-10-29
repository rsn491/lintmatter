---
name: architecture
description: Use when adding, moving or reviewing code in lintmatter's src/ - module layout, which layers may import which, and step-by-step guides for adding a rule or a setting.
---

# lintmatter architecture

## Layout

Layers, top to bottom. A module may import only from layers below it.

- `src/main.rs` — entry point only; calls `cli::run`.
- `src/cli/` — `args.rs` parses flags into `Flags`, `usage.rs` prints `--help`,
  `run.rs` validates a `Cli` from the merged settings and executes it: discovery,
  linting, then `--strict` (before the summary) and `--quiet` (after it), then a
  reporter. `run` takes its writers as arguments so tests drive the whole CLI
  in-process.
- `src/config/` — `mod.rs` holds `Settings` (what the flags or the file asked
  for, budgets `None` when unset), `resolve` (flags over file over defaults)
  and `Source`; `file.rs` is the `.lintmatter.yaml` loader and its discovery
  walk. Only budgets, `exclude` and `rules` are configurable there; run
  behavior (`strict`, `quiet`, `format`, `color`) is flag-only — see the module
  doc comment.
- `src/report/` — the `Report` trait, `summarize`, `TextReporter`,
  `JsonReporter`; `format.rs` holds `plural`.
- `src/lint/` — `engine.rs`: `Linter` and `LintOptions`; `context.rs`:
  `FileContext`, what a rule may read; `sink.rs`: `FindingSink` and the per-rule
  tally; `score.rs`: the 0–100 score; `rule.rs`: `Rule`, `RuleMeta`, `Part`;
  `rules/`: one struct per rule id, and `rules::all()`, the registry;
  `testing.rs`: test fixtures.
- `src/source/` — `discover.rs` turns paths into `Target`s, pruning dependency
  directories; `parse.rs` splits front matter from body, keeping line numbers
  (`Document::mapping` is `Some` only when the block is usable, which is how
  rules short-circuit on a malformed one); `fence.rs` tracks fenced code
  blocks; `paths.rs` holds `to_slash` and `clean_path`.
- `src/tokens.rs` — heuristic token estimator behind the `Counter` trait.
- `src/model/` — shared data with no crate dependencies: `Kind`, `Target`,
  `Severity`, `Finding`, `Counts`, `FileResult`; `budgets.rs` holds
  `Budgets<T>`, the defaults, and `FIELDS`, each budget's flag/key name.
- `src/lib.rs` — the module list. The crate is a library as well as a binary so
  `web/` can read the default budgets (via `config::DEFAULT_*`) rather than
  restating numbers that would drift.
- `tests/cli.rs` — end-to-end tests over `testdata/`; unit tests live beside their
  code, and a rule's tests beside the rule.
- `web/` — the `lintmatter-web` crate: an axum server that clones a GitHub repo
  and runs the `lintmatter` binary over it. It listens on `HOST`/`PORT`
  (default `127.0.0.1:3000`); a container host wants `HOST=0.0.0.0`. `lint.rs`'s
  `Budgets` turns the form's token budgets into `--max-*` flags; `main.rs`
  substitutes the defaults into `index.html`'s `{{BUDGET_SETTINGS}}` at render
  time, so the form opens on the linter's own numbers.

## Layering

Allowed imports: `cli` → anything below; `config` → `lint` (only `RULES`, to
validate `rules:`), `source`, `model`; `report` → `model`; `lint` → `source`,
`tokens`, `model`; `source` → `model`; `model` → nothing. Nothing imports `cli`,
`lint` never imports `config`, and `report` never imports `lint`: the engine
takes plain `LintOptions`, and reporters see only `model` types.

## Adding a rule

1. Add a struct implementing `Rule` in the matching `src/lint/rules/` module.
   Its `const META: RuleMeta` is the only place its id, score `Part` and file
   kinds are written (`SKILL_ONLY` or `ALL_KINDS`). Emit through
   `FindingSink::error` or `warn` with the rule's true severity; `--strict` is
   the CLI's to apply.
2. In `check`, call `FindingSink::applies` where the rule's precondition holds,
   ahead of the condition that fires it, so the rule lands in the score's
   denominator whether or not it fires. Call it only where the rule really had
   something to judge: a check skipped for this file must not count against
   it. `error`/`warn` imply it, and so does `grade`, for a rule scored on a
   scale rather than pass/fail.
3. Register it in `rules::all()` **at the position it should be reported**.
   That order is the only place report order is written down: it drives
   `RULES`, `--list-rules`, `--disable` validation, the config file's `rules:`
   mapping, and the order findings appear in.
4. Add a test beside the rule asserting the exact rule ids its fixtures produce
   (`testing::assert_skill_cases`).
5. Document it in the README's rule table.

## Adding a setting

A token budget: a `Field` in `model/budgets.rs`'s `FIELDS` and a field on
`Budgets` (plus its line in `get_mut`, `DEFAULT`, `or` and `unwrap_or`). The
flag, the config key, merging and the help text all follow from that. The web
form keeps its own copy: a field on `web/src/lint.rs`'s `Budgets` with its
entry in `entries`, and an input in `index.html` whose `id` is that field name.
Any other config-file setting: a field on `config::Settings`, its arm in
`cli/args.rs`'s `parse_flags` and in `config/file.rs`'s `parse` and
`known_keys`, and its merge in `resolve`. Run-behavior settings (how a result
is reported, not what counts as a violation) belong on `Flags` alone. Document
either kind in the README's flag table.
