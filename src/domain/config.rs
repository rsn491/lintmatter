//! The thresholds and switches that shape a lint run.

use super::Kind;

/// Default token budgets. `Config::default` and the CLI's flag defaults both
/// read these, so the two cannot drift apart.
pub const DEFAULT_MAX_AGENTS_TOKENS: i64 = 2500;
pub const DEFAULT_MAX_SKILL_TOKENS: i64 = 5000;
pub const DEFAULT_MAX_SKILL_NAME_TOKENS: i64 = 16;
pub const DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS: i64 = 100;

/// Rules that are off until a config file's `rules:` mapping turns them on.
/// Not part of `Config::default`: that keeps every rule running so the linter
/// can still be exercised on it; the settings layer applies this list. Unknown
/// front-matter keys are usually a tool's own extension, not a mistake.
pub const DEFAULT_DISABLED_RULES: &[&str] = &["frontmatter.unknown-key"];

/// Holds the thresholds and switches that shape a run.
#[derive(Debug, Clone)]
pub struct Config {
    /// Body token budgets, one per file kind. Zero disables the check for
    /// that kind.
    pub max_agents_tokens: i64,
    pub max_skill_tokens: i64,
    /// Skill-only budgets. Zero disables the check.
    pub max_skill_name_tokens: i64,
    pub max_skill_description_tokens: i64,
    /// Rule ids to skip.
    pub disabled: Vec<String>,
    /// Treat warnings as errors.
    pub strict: bool,
}

/// Zero means "check disabled", so a derived `Default` would hand back a
/// linter that silently enforces nothing. Spell the real budgets out instead,
/// so the value reached by accident is the safe one.
impl Default for Config {
    fn default() -> Self {
        Config {
            max_agents_tokens: DEFAULT_MAX_AGENTS_TOKENS,
            max_skill_tokens: DEFAULT_MAX_SKILL_TOKENS,
            max_skill_name_tokens: DEFAULT_MAX_SKILL_NAME_TOKENS,
            max_skill_description_tokens: DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
            disabled: Vec::new(),
            strict: false,
        }
    }
}

impl Config {
    pub(crate) fn content_limit(&self, kind: Kind) -> i64 {
        if kind == Kind::Agents {
            self.max_agents_tokens
        } else {
            self.max_skill_tokens
        }
    }
}
