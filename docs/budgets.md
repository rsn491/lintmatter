# Default budgets

lintmatter's default token budgets follow guidance published for agent
instruction files. This page records where each number comes from, so a change
to that guidance can be checked against the defaults.

| Budget | Default | Source | Why |
| --- | --- | --- | --- |
| `SKILL.md` body | 5000 tokens | [Agent Skills spec](https://agentskills.io/specification), [Anthropic skills overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview) | The spec recommends keeping a skill's instructions under 5000 tokens; Anthropic budgets a loaded skill at "under 5k tokens" |
| Skill `name` | 16 tokens | [Anthropic skills overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview) | A skill's `name` and `description` are loaded for every skill at startup, which Anthropic counts at about 100 tokens per skill |
| Skill `description` | 100 tokens | [Anthropic skills overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview) | Same always-loaded metadata as `name` |
| `AGENTS.md` body | 2500 tokens | [Claude Code memory docs](https://code.claude.com/docs/en/memory), [Codex AGENTS.md guide](https://developers.openai.com/codex/guides/agents-md) | Claude Code says to target under 200 lines per instruction file, which is about 2.5k tokens. That is well inside Codex's 32 KiB cap on all AGENTS.md files combined (`project_doc_max_bytes`) |

The token budgets for `name` and `description` sit inside the spec's hard limits of
64 and 1024 characters. Those limits are checked separately, by the
`name.length` and `description.length` rules.

To change a budget for one run, pass its flag (see the
[flag table](../README.md#flags)). To change it for a repository, set it in a
[`.lintmatter.yaml`](../.lintmatter.yaml) file. `0` disables a budget.
