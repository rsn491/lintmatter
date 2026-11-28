# Known front-matter keys

`frontmatter.unknown-key` (off by default) warns about any `SKILL.md`
front-matter key that isn't listed here. The list lives in `KNOWN_KEYS` in
[`src/domain/lint/rules/frontmatter.rs`](../src/domain/lint/rules/frontmatter.rs).

## Agent Skills spec

The fields the [Agent Skills spec](https://agentskills.io/specification#frontmatter)
defines:

- `name`
- `description`
- `license`
- `compatibility`
- `metadata`
- `allowed-tools`

## Claude Code extensions

Fields [Claude Code](https://code.claude.com/docs/en/skills) adds for its skills:

- `when_to_use`
- `argument-hint`
- `arguments`
- `disable-model-invocation`
- `user-invocable`
- `disallowed-tools`
- `model`
- `effort`
- `context`
- `agent`
- `background`
- `hooks`
- `paths`
- `shell`
