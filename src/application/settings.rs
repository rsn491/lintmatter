//! Merges the settings a run was given into the [`Config`] the linter
//! applies. Two sources supply the same shape of [`Settings`]: the command
//! line and the project's config file. The flags win over the file, and the
//! file wins over the built-in defaults, so a config file sets the project's
//! baseline without preventing a one-off override on the command line.

use std::collections::HashSet;

use crate::domain::Config;
use crate::domain::config::{
    DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS, DEFAULT_MAX_SKILL_NAME_TOKENS,
    DEFAULT_MAX_SKILL_TOKENS,
};
use crate::domain::lint::RULES;

/// The settings one source asked for. Every scalar is optional: absent
/// means "leave it to the next source or the default", which is what makes
/// merging unambiguous.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub max_agents_tokens: Option<i64>,
    pub max_skill_tokens: Option<i64>,
    pub max_skill_name_tokens: Option<i64>,
    pub max_skill_description_tokens: Option<i64>,
    /// Globs of paths to skip.
    pub excludes: Vec<String>,
    /// Rule ids to switch off.
    pub disabled: Vec<String>,
}

/// What a run actually uses once every source has been merged.
#[derive(Debug, Clone)]
pub struct RunSettings {
    pub config: Config,
    pub excludes: Vec<String>,
    pub paths: Vec<String>,
}

/// Merges flags over config-file settings over the built-in defaults. Lists
/// accumulate instead of overriding: `--exclude` and `--disable` add to
/// whatever the file already asked for, since narrowing a run further on the
/// command line is the common case. `strict` and `paths` come from the
/// command line alone: the config file has no say over them.
pub fn resolve(flags: Settings, file: Settings, strict: bool, paths: Vec<String>) -> RunSettings {
    let mut excludes = file.excludes;
    excludes.extend(flags.excludes);
    let mut disabled = file.disabled;
    disabled.extend(flags.disabled);

    RunSettings {
        config: Config {
            max_agents_tokens: flags
                .max_agents_tokens
                .or(file.max_agents_tokens)
                .unwrap_or(DEFAULT_MAX_AGENTS_TOKENS),
            max_skill_tokens: flags
                .max_skill_tokens
                .or(file.max_skill_tokens)
                .unwrap_or(DEFAULT_MAX_SKILL_TOKENS),
            max_skill_name_tokens: flags
                .max_skill_name_tokens
                .or(file.max_skill_name_tokens)
                .unwrap_or(DEFAULT_MAX_SKILL_NAME_TOKENS),
            max_skill_description_tokens: flags
                .max_skill_description_tokens
                .or(file.max_skill_description_tokens)
                .unwrap_or(DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS),
            disabled,
            strict,
        },
        excludes,
        paths: if paths.is_empty() {
            vec![".".to_string()]
        } else {
            paths
        },
    }
}

/// `--disable` named rules that do not exist.
#[derive(Debug, thiserror::Error)]
#[error("unknown rule {} in --disable: run --list-rules to see them all", quote_all(.0))]
pub struct UnknownRules(pub Vec<String>);

fn quote_all(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|r| format!("{r:?}")).collect();
    quoted.join(", ")
}

/// Rejects typos in `--disable` rather than silently doing nothing.
pub fn check_rule_names(rules: &[String]) -> Result<(), UnknownRules> {
    let known: HashSet<&str> = RULES.iter().copied().collect();
    let mut unknown: Vec<String> = rules
        .iter()
        .filter(|r| !known.contains(r.as_str()))
        .cloned()
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort();
    Err(UnknownRules(unknown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_fills_in_what_the_flags_leave_unset() {
        let file = Settings {
            max_skill_tokens: Some(7),
            ..Default::default()
        };
        let resolved = resolve(Settings::default(), file, false, vec![]);
        assert_eq!(resolved.config.max_skill_tokens, 7);
        assert_eq!(resolved.config.max_agents_tokens, DEFAULT_MAX_AGENTS_TOKENS);
        assert_eq!(resolved.paths, vec![".".to_string()]);
    }

    #[test]
    fn flags_win_and_lists_accumulate() {
        let flags = Settings {
            max_skill_tokens: Some(9),
            excludes: vec!["b".into()],
            disabled: vec!["name.format".into()],
            ..Default::default()
        };
        let file = Settings {
            max_skill_tokens: Some(7),
            excludes: vec!["a".into()],
            disabled: vec!["name.length".into()],
            ..Default::default()
        };
        let resolved = resolve(flags, file, true, vec!["x".into()]);
        assert_eq!(resolved.config.max_skill_tokens, 9);
        assert_eq!(resolved.excludes, vec!["a", "b"]);
        assert_eq!(resolved.config.disabled, vec!["name.length", "name.format"]);
        assert!(resolved.config.strict);
        assert_eq!(resolved.paths, vec!["x"]);
    }

    #[test]
    fn unknown_rules_are_sorted_and_quoted() {
        assert!(check_rule_names(&["name.format".into()]).is_ok());
        let err = check_rule_names(&["zz".into(), "aa".into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown rule \"aa\", \"zz\" in --disable: run --list-rules to see them all"
        );
    }
}
