# Working in lintmatter

`lintmatter` lints agent instruction files. It is a Rust CLI; the YAML front
matter is parsed with `saphyr`, everything else is hand-rolled.

## Checks before pushing

```sh
cargo fmt --check       # must report nothing
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Layout

Layers, top to bottom; a module imports only from layers below it:
`main.rs` → `cli/` (flags, help, orchestration) → `config/` (settings merge,
`.lintmatter.yaml`) → `report/` (text, JSON) → `lint/` (engine, sink, score,
`rules/`) → `source/` (discovery, parsing) and `tokens.rs` → `model/` (shared
types, `Budgets`). `web/` is a separate crate that runs the binary.

Before adding, moving or reviewing code in `src/`, read
`.claude/skills/architecture/SKILL.md`: the full layout, the exact import rules,
and the steps for adding a rule or a setting.

## Conventions

- Prefer errors for anything that breaks a file's contract with the runtime, and
  warnings for stylistic mismatches. Warnings alone exit 0.
- Findings come out in registry order because the rules run in that order, so
  output never depends on the order checks happen to execute in. Nothing sorts
  them afterwards; keep `rules::all()` in report order instead.
- A file's score rates only the checks that ran on it. A part with nothing to
  judge drops out of the mean rather than scoring a free 100.
- Comments explain why a check exists, not what the code does.
- Requested usage goes to stdout; diagnostics (including usage after a flag
  error) go to stderr.
- An unreadable file is reported and skipped, not fatal; discovery errors still
  exit 2.
