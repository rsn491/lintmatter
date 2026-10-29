//! The token budgets: the one setting lintmatter has four of.
//!
//! A budget is spelled the same way everywhere it is written down -- as a
//! flag, as a config-file key, and in the usage text -- so [`FIELDS`] holds
//! that spelling once and each of those places iterates it, rather than each
//! restating the four names and drifting apart.

use super::Kind;

/// Default token budgets. `Budgets::DEFAULT` and the usage text both read
/// these, and the web crate starts its form from them.
pub const DEFAULT_MAX_AGENTS_TOKENS: i64 = 2500;
pub const DEFAULT_MAX_SKILL_TOKENS: i64 = 5000;
pub const DEFAULT_MAX_SKILL_NAME_TOKENS: i64 = 16;
pub const DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS: i64 = 100;

/// How one budget is named and described outside the code.
#[derive(Debug)]
pub struct Field {
    /// The flag name without its dashes, which is also the config-file key.
    pub key: &'static str,
    /// What the budget limits, for the usage text.
    pub help: &'static str,
    pub default: i64,
}

/// Every budget's spelling, in the order of [`Budgets`]' fields, which is
/// also the order the usage text lists them.
pub static FIELDS: [Field; 4] = [
    Field {
        key: "max-agents-tokens",
        help: "token budget for AGENTS.md content",
        default: DEFAULT_MAX_AGENTS_TOKENS,
    },
    Field {
        key: "max-skill-tokens",
        help: "token budget for SKILL.md content",
        default: DEFAULT_MAX_SKILL_TOKENS,
    },
    Field {
        key: "max-skill-name-tokens",
        help: "token budget for a skill's name",
        default: DEFAULT_MAX_SKILL_NAME_TOKENS,
    },
    Field {
        key: "max-skill-description-tokens",
        help: "token budget for a skill's description",
        default: DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    },
];

/// One value per token budget. `Budgets<Option<i64>>` is what a flag set or a
/// config file asked for, where `None` leaves the budget to the next source;
/// `Budgets<i64>` is what a run enforces, where zero disables the check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budgets<T> {
    pub agents: T,
    pub skill: T,
    pub skill_name: T,
    pub skill_description: T,
}

impl<T> Budgets<T> {
    /// The slot a flag or config key names, or `None` when `key` is not a
    /// budget.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        let slots = [
            &mut self.agents,
            &mut self.skill,
            &mut self.skill_name,
            &mut self.skill_description,
        ];
        FIELDS
            .iter()
            .zip(slots)
            .find(|(field, _)| field.key == key)
            .map(|(_, slot)| slot)
    }
}

impl Budgets<i64> {
    /// Zero means "check disabled", so a derived `Default` would hand back a
    /// linter that silently enforces nothing. This is the value to reach for
    /// instead, so the one reached by accident is the safe one.
    pub const DEFAULT: Self = Budgets {
        agents: DEFAULT_MAX_AGENTS_TOKENS,
        skill: DEFAULT_MAX_SKILL_TOKENS,
        skill_name: DEFAULT_MAX_SKILL_NAME_TOKENS,
        skill_description: DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    };

    /// The budget for a file's body, which depends on what kind of file it is.
    pub fn content_limit(&self, kind: Kind) -> i64 {
        match kind {
            Kind::Agents => self.agents,
            Kind::Skill => self.skill,
        }
    }
}

impl Budgets<Option<i64>> {
    /// Each budget from `self` where set, otherwise from `fallback`.
    pub fn or(self, fallback: Self) -> Self {
        Budgets {
            agents: self.agents.or(fallback.agents),
            skill: self.skill.or(fallback.skill),
            skill_name: self.skill_name.or(fallback.skill_name),
            skill_description: self.skill_description.or(fallback.skill_description),
        }
    }

    /// Fills every budget nothing asked for from `defaults`.
    pub fn unwrap_or(self, defaults: Budgets<i64>) -> Budgets<i64> {
        Budgets {
            agents: self.agents.unwrap_or(defaults.agents),
            skill: self.skill.unwrap_or(defaults.skill),
            skill_name: self.skill_name.unwrap_or(defaults.skill_name),
            skill_description: self.skill_description.unwrap_or(defaults.skill_description),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `get_mut` pairs FIELDS with the struct's fields by position, so the
    /// two orders must agree.
    #[test]
    fn fields_line_up_with_the_struct() {
        let mut b = Budgets::DEFAULT;
        for field in &FIELDS {
            assert_eq!(
                *b.get_mut(field.key).unwrap(),
                field.default,
                "{}",
                field.key
            );
        }
        assert!(b.get_mut("max-nothing-tokens").is_none());
    }

    #[test]
    fn merge_takes_the_first_source_that_set_a_budget() {
        let flags = Budgets {
            skill: Some(1),
            ..Default::default()
        };
        let file = Budgets {
            skill: Some(2),
            agents: Some(3),
            ..Default::default()
        };
        let got = flags.or(file).unwrap_or(Budgets::DEFAULT);
        assert_eq!(got.skill, 1);
        assert_eq!(got.agents, 3);
        assert_eq!(got.skill_name, DEFAULT_MAX_SKILL_NAME_TOKENS);
    }

    #[test]
    fn content_limit_follows_the_kind() {
        let b = Budgets::DEFAULT;
        assert_eq!(b.content_limit(Kind::Agents), DEFAULT_MAX_AGENTS_TOKENS);
        assert_eq!(b.content_limit(Kind::Skill), DEFAULT_MAX_SKILL_TOKENS);
    }
}
