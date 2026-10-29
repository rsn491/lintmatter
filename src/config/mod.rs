//! What a run checks for, wherever it was asked for: the command line, the
//! YAML config file a repository can use to pin settings once instead of
//! repeating flags on every invocation, and the built-in defaults.
//!
//! Run-behavior flags (`--strict`, `--quiet`, `--format`, `--color`) are
//! deliberately not configurable here: they shape how a single invocation
//! reports its findings, not what the repository considers a violation, so
//! they stay flag-only.
//!
//! Every file setting mirrors the flag of the same name. Flags win over the
//! file, and the file wins over the built-in defaults, so a config file sets
//! the project's baseline without preventing a one-off override on the
//! command line.

mod file;

use std::path::{Path, PathBuf};

use crate::model::Budgets;

pub use crate::model::{
    DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS, DEFAULT_MAX_SKILL_NAME_TOKENS,
    DEFAULT_MAX_SKILL_TOKENS,
};
pub use file::{FILE_NAMES, discover, load, parse};

/// The settings one source -- the flags or a config file -- asked for. Every
/// budget is optional: absent means "leave it to the next source", which is
/// what makes merging unambiguous.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub budgets: Budgets<Option<i64>>,
    /// Globs of paths to skip.
    pub excludes: Vec<String>,
    /// Rule ids switched off.
    pub disabled: Vec<String>,
}

/// The settings a run actually uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub budgets: Budgets<i64>,
    pub excludes: Vec<String>,
    pub disabled: Vec<String>,
}

/// Merges flags over config-file settings over the built-in defaults. Lists
/// accumulate instead of overriding: `--exclude` and `--disable` add to
/// whatever the file already asked for, since narrowing a run further on the
/// command line is the common case.
pub fn resolve(flags: Settings, file: Settings) -> Resolved {
    let mut excludes = file.excludes;
    excludes.extend(flags.excludes);
    let mut disabled = file.disabled;
    disabled.extend(flags.disabled);
    Resolved {
        budgets: flags.budgets.or(file.budgets).unwrap_or(Budgets::DEFAULT),
        excludes,
        disabled,
    }
}

/// Which config file a run reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The file named by `--config`.
    Explicit(PathBuf),
    /// The nearest config file at or above the working directory, if any.
    Discover,
    /// None at all, for `--no-config`.
    Skip,
}

/// Reads the config file `source` names. Finding nothing to read is not an
/// error: it means the flags and defaults decide everything.
pub fn load_from(source: &Source, cwd: &Path) -> Result<Settings, String> {
    match source {
        Source::Explicit(path) => load(path),
        Source::Discover => match discover(cwd) {
            Some(path) => load(&path),
            None => Ok(Settings::default()),
        },
        Source::Skip => Ok(Settings::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_win_over_the_file_and_lists_accumulate() {
        let flags = Settings {
            budgets: Budgets {
                skill: Some(1),
                ..Default::default()
            },
            excludes: vec!["b".to_string()],
            disabled: vec!["name.format".to_string()],
        };
        let file = Settings {
            budgets: Budgets {
                skill: Some(2),
                agents: Some(3),
                ..Default::default()
            },
            excludes: vec!["a".to_string()],
            disabled: vec!["name.length".to_string()],
        };
        let got = resolve(flags, file);
        assert_eq!(got.budgets.skill, 1);
        assert_eq!(got.budgets.agents, 3);
        assert_eq!(got.budgets.skill_name, DEFAULT_MAX_SKILL_NAME_TOKENS);
        assert_eq!(got.excludes, vec!["a", "b"]);
        assert_eq!(got.disabled, vec!["name.length", "name.format"]);
    }

    #[test]
    fn config_is_discovered_from_the_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".lintmatter.yaml"), "max-skill-tokens: 7\n").unwrap();
        let nested = dir.path().join("skills/deep");
        std::fs::create_dir_all(&nested).unwrap();

        let cfg = load_from(&Source::Discover, &nested).unwrap();
        assert_eq!(cfg.budgets.skill, Some(7));
        let resolved = resolve(Settings::default(), cfg);
        assert_eq!(resolved.budgets.skill, 7);
        assert_eq!(resolved.budgets.agents, DEFAULT_MAX_AGENTS_TOKENS);

        let skipped = load_from(&Source::Skip, &nested).unwrap();
        assert_eq!(skipped, Settings::default());
    }
}
