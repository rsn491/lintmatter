//! The YAML config file a repository can use to pin a run's budgets,
//! excludes and rules once instead of repeating flags on every invocation.
//!
//! Run-behavior flags (`--strict`, `--quiet`, `--format`, `--color`) are
//! deliberately not configurable here: they shape how a single invocation
//! reports its findings, not what the repository considers a violation, so
//! they stay flag-only.
//!
//! Every file setting mirrors the flag of the same name; how the two are
//! merged is [`crate::application::settings::resolve`]'s business.

use std::io;
use std::path::{Path, PathBuf};

use saphyr::{LoadableYamlNode, MarkedYaml, YamlData};

use crate::application::Settings;
use crate::domain::document::scalar_string;
use crate::domain::lint::RULES;

/// File names looked for when `--config` is not given, in priority order.
pub const FILE_NAMES: &[&str] = &[".lintmatter.yaml", ".lintmatter.yml"];

/// Every setting a config file may carry, in the order the usage text lists
/// them. Backs both parsing and the "unknown setting" message.
const KNOWN_KEYS: &[&str] = &[
    "max-agents-tokens",
    "max-skill-tokens",
    "max-skill-name-tokens",
    "max-skill-description-tokens",
    "exclude",
    "rules",
];

/// Returns the config file governing `start`: the first of [`FILE_NAMES`]
/// found in `start` or any ancestor directory. Walking up means running
/// lintmatter from a subdirectory still picks up the repository's settings.
pub fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .flat_map(|dir| FILE_NAMES.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Why a config file could not be used.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{path}:{line}: {problem}")]
    Invalid {
        path: String,
        line: usize,
        problem: Problem,
    },
}

/// What is wrong at one line of a config file.
#[derive(Debug, thiserror::Error)]
pub enum Problem {
    #[error("invalid YAML: {0}")]
    Yaml(String),
    #[error("config must be a mapping of settings to values")]
    NotMapping,
    #[error("setting names must be strings")]
    NonStringKey,
    #[error("unknown setting {0:?}: want one of {keys}", keys = KNOWN_KEYS.join(", "))]
    UnknownSetting(String),
    #[error("rules must be a mapping of rule id to true or false")]
    RulesNotMapping,
    #[error("rule ids must be strings")]
    NonStringRuleId,
    #[error("unknown rule {0:?}: run --list-rules to see them all")]
    UnknownRule(String),
    #[error("{key} must be an integer")]
    NotInteger { key: String },
    #[error("invalid value {text:?} for {key}: not an integer")]
    BadInteger { key: String, text: String },
    #[error("{key} must be zero or more (0 disables the check)")]
    Negative { key: String },
    #[error("{key} must be true or false")]
    NotBoolean { key: String },
    #[error("invalid value {text:?} for {key}: want true or false")]
    BadBoolean { key: String, text: String },
    #[error("{key} must be a non-empty string")]
    EmptyString { key: String },
}

fn invalid(path: &str, line: usize, problem: Problem) -> ConfigError {
    ConfigError::Invalid {
        path: path.to_string(),
        line,
        problem,
    }
}

/// Reads and parses a config file.
pub fn load(path: &Path) -> Result<Settings, ConfigError> {
    let display = path.to_string_lossy().replace('\\', "/");
    let src = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&src, &display)
}

/// Parses config text. `path` only labels the error messages.
pub fn parse(src: &str, path: &str) -> Result<Settings, ConfigError> {
    let cfg = Settings::default();

    let docs = MarkedYaml::load_from_str(src).map_err(|e| {
        invalid(
            path,
            e.marker().line(),
            Problem::Yaml(e.info().trim().to_string()),
        )
    })?;
    // A file that is empty or holds only comments parses to no document, and
    // one holding a bare `---` to a null document. Both mean "no settings".
    let Some(root) = docs.into_iter().next() else {
        return Ok(cfg);
    };
    if is_null(&root) {
        return Ok(cfg);
    }
    let YamlData::Mapping(map) = &root.data else {
        return Err(invalid(path, root.span.start.line(), Problem::NotMapping));
    };

    map.iter().try_fold(cfg, |mut cfg, (key, value)| {
        let line = key.span.start.line();
        let Some(name) = scalar_string(&key.data) else {
            return Err(invalid(path, line, Problem::NonStringKey));
        };
        match name.as_str() {
            "max-agents-tokens" => cfg.max_agents_tokens = Some(budget(path, &name, value)?),
            "max-skill-tokens" => cfg.max_skill_tokens = Some(budget(path, &name, value)?),
            "max-skill-name-tokens" => {
                cfg.max_skill_name_tokens = Some(budget(path, &name, value)?);
            }
            "max-skill-description-tokens" => {
                cfg.max_skill_description_tokens = Some(budget(path, &name, value)?);
            }
            "exclude" => cfg.excludes = string_list(path, &name, value)?,
            "rules" => cfg.disabled = rules(path, value)?,
            _ => return Err(invalid(path, line, Problem::UnknownSetting(name))),
        }
        Ok(cfg)
    })
}

/// Reads `rules:`, a mapping of rule id to whether it runs. Only the ids
/// switched off are returned; naming a rule that lintmatter does not have is an
/// error rather than a silent no-op, the same way `--disable` rejects typos.
fn rules(path: &str, node: &MarkedYaml) -> Result<Vec<String>, ConfigError> {
    let YamlData::Mapping(map) = &node.data else {
        return Err(invalid(
            path,
            node.span.start.line(),
            Problem::RulesNotMapping,
        ));
    };

    map.iter()
        .map(|(key, value)| {
            let key_line = key.span.start.line();
            let Some(rule) = scalar_string(&key.data) else {
                return Err(invalid(path, key_line, Problem::NonStringRuleId));
            };
            if !RULES.contains(&rule.as_str()) {
                return Err(invalid(path, key_line, Problem::UnknownRule(rule)));
            }
            let enabled = boolean(path, &format!("rules.{rule}"), value)?;
            Ok((!enabled).then_some(rule))
        })
        .filter_map(Result::transpose)
        .collect()
}

/// Reads a token budget. Zero disables the check it belongs to, matching the
/// flags.
fn budget(path: &str, key: &str, node: &MarkedYaml) -> Result<i64, ConfigError> {
    let line = node.span.start.line();
    let key = key.to_string();
    let Some(text) = scalar_string(&node.data) else {
        return Err(invalid(path, line, Problem::NotInteger { key }));
    };
    let Ok(n) = text.parse::<i64>() else {
        return Err(invalid(path, line, Problem::BadInteger { key, text }));
    };
    if n < 0 {
        return Err(invalid(path, line, Problem::Negative { key }));
    }
    Ok(n)
}

/// Reads a boolean. YAML 1.2 only spells these `true` and `false`, but config
/// files are hand-written, so the usual `yes`/`no` and `on`/`off` spellings
/// are accepted rather than reported as a type error.
fn boolean(path: &str, key: &str, node: &MarkedYaml) -> Result<bool, ConfigError> {
    let line = node.span.start.line();
    let key = key.to_string();
    let Some(text) = scalar_string(&node.data) else {
        return Err(invalid(path, line, Problem::NotBoolean { key }));
    };
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Ok(true),
        "false" | "no" | "off" => Ok(false),
        _ => Err(invalid(path, line, Problem::BadBoolean { key, text })),
    }
}

fn string(path: &str, key: &str, node: &MarkedYaml) -> Result<String, ConfigError> {
    match scalar_string(&node.data) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(invalid(
            path,
            node.span.start.line(),
            Problem::EmptyString {
                key: key.to_string(),
            },
        )),
    }
}

/// Reads a list of strings, accepting a lone scalar as a one-element list so
/// `exclude: testdata` works as well as the sequence form.
fn string_list(path: &str, key: &str, node: &MarkedYaml) -> Result<Vec<String>, ConfigError> {
    match &node.data {
        YamlData::Sequence(items) => items.iter().map(|item| string(path, key, item)).collect(),
        _ => Ok(vec![string(path, key, node)?]),
    }
}

fn is_null(node: &MarkedYaml) -> bool {
    matches!(&node.data, YamlData::Value(saphyr::Scalar::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn parse_str(src: &str) -> Result<Settings, String> {
        parse(src, ".lintmatter.yaml").map_err(|e| e.to_string())
    }

    #[test]
    fn parses_every_setting() {
        // Given
        let src = r#"
max-agents-tokens: 1200
max-skill-tokens: 0
max-skill-name-tokens: 8
max-skill-description-tokens: 64
exclude:
  - testdata
  - "examples/**"
rules:
  name.dir-mismatch: false
  frontmatter.unknown-key: off
  tokens.content: true
"#;

        // When
        let got = parse_str(src).unwrap();

        // Then
        assert_eq!(
            got,
            Settings {
                max_agents_tokens: Some(1200),
                max_skill_tokens: Some(0),
                max_skill_name_tokens: Some(8),
                max_skill_description_tokens: Some(64),
                excludes: vec!["testdata".to_string(), "examples/**".to_string()],
                // Only the rules switched off are collected, in file order.
                disabled: vec![
                    "name.dir-mismatch".to_string(),
                    "frontmatter.unknown-key".to_string(),
                ],
            }
        );
    }

    #[test]
    fn empty_documents_yield_no_settings() {
        // Given
        let sources = ["", "\n\n", "# just a comment\n", "---\n"];

        for src in sources {
            // When
            let got = parse_str(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));

            // Then
            assert_eq!(got, Settings::default(), "{src:?}");
        }
    }

    #[test]
    fn scalar_shorthand_accepts_a_lone_exclude() {
        // Given
        let src = "exclude: testdata\n";

        // When
        let got = parse_str(src).unwrap();

        // Then
        assert_eq!(got.excludes, vec!["testdata".to_string()]);
    }

    #[test]
    fn errors_point_at_the_offending_line() {
        // Given
        let cases: &[(&str, &str, &str)] = &[
            (
                "unknown setting",
                "max-skill-tokens: 10\nmax-skil-tokens: 10\n",
                ".lintmatter.yaml:2: unknown setting \"max-skil-tokens\"",
            ),
            (
                "not an integer",
                "max-agents-tokens: lots\n",
                ".lintmatter.yaml:1: invalid value \"lots\" for max-agents-tokens",
            ),
            (
                "negative budget",
                "\nmax-skill-tokens: -1\n",
                ".lintmatter.yaml:2: max-skill-tokens must be zero or more",
            ),
            (
                "budget is a list",
                "max-skill-tokens:\n  - 10\n",
                ".lintmatter.yaml:2: max-skill-tokens must be an integer",
            ),
            (
                "empty string",
                "exclude: \"\"\n",
                ".lintmatter.yaml:1: exclude must be a non-empty string",
            ),
            (
                "exclude entry is a mapping",
                "exclude:\n  - drop: true\n",
                ".lintmatter.yaml:2: exclude must be a non-empty string",
            ),
            (
                "rules is not a mapping",
                "rules:\n  - name.format\n",
                ".lintmatter.yaml:2: rules must be a mapping",
            ),
            (
                "unknown rule",
                "rules:\n  name.format: true\n  no.such.rule: false\n",
                ".lintmatter.yaml:3: unknown rule \"no.such.rule\"",
            ),
            (
                "rule value is not a boolean",
                "rules:\n  name.format: maybe\n",
                ".lintmatter.yaml:2: invalid value \"maybe\" for rules.name.format",
            ),
            (
                "not a mapping",
                "- max-skill-tokens\n",
                ".lintmatter.yaml:1: config must be a mapping",
            ),
            (
                "invalid yaml",
                "max-skill-tokens: 1\n  exclude: true\n",
                "invalid YAML",
            ),
        ];
        for (name, src, want) in cases {
            // When
            let err = parse_str(src).unwrap_err();

            // Then
            assert!(err.contains(want), "{name}: got {err:?}, want {want:?}");
            assert!(!err.contains('\n'), "{name}: message should be one line");
        }
    }

    #[test]
    fn load_reads_a_file() {
        // Given
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lintmatter.yaml");
        fs::write(&path, "max-skill-tokens: 42\n").unwrap();
        let bad_dir = tempfile::tempdir().unwrap();
        let bad_path = bad_dir.path().join(".lintmatter.yaml");
        fs::write(&bad_path, "max-skill-tokens: nope\n").unwrap();

        // When
        let got = load(&path).unwrap();
        let missing_err = load(&dir.path().join("nope.yaml")).unwrap_err().to_string();
        let invalid_err = load(&bad_path).unwrap_err().to_string();

        // Then
        assert_eq!(got.max_skill_tokens, Some(42));
        assert!(missing_err.contains("cannot read config"), "{missing_err}");
        // Errors name the file they came from.
        assert!(invalid_err.contains(".lintmatter.yaml:1:"), "{invalid_err}");
    }

    #[test]
    fn discover_walks_up_from_the_starting_directory() {
        // Given
        let empty = tempfile::tempdir().unwrap();
        let empty_nested = empty.path().join("skills/deep");
        fs::create_dir_all(&empty_nested).unwrap();

        let yml = tempfile::tempdir().unwrap();
        let yml_nested = yml.path().join("skills/deep");
        fs::create_dir_all(&yml_nested).unwrap();
        let root_cfg = yml.path().join(".lintmatter.yml");
        fs::write(&root_cfg, "max-skill-tokens: 1\n").unwrap();

        let both = tempfile::tempdir().unwrap();
        fs::write(both.path().join(".lintmatter.yml"), "max-skill-tokens: 1\n").unwrap();
        let preferred = both.path().join(".lintmatter.yaml");
        fs::write(&preferred, "max-skill-tokens: 1\n").unwrap();

        let ancestors = tempfile::tempdir().unwrap();
        let ancestors_nested = ancestors.path().join("skills/deep");
        fs::create_dir_all(&ancestors_nested).unwrap();
        fs::write(
            ancestors.path().join(".lintmatter.yaml"),
            "max-skill-tokens: 1\n",
        )
        .unwrap();
        let nested_cfg = ancestors_nested.join(".lintmatter.yaml");
        fs::write(&nested_cfg, "max-skill-tokens: 2\n").unwrap();

        // When
        let none = discover(&empty_nested);
        let from_ancestor = discover(&yml_nested);
        let yaml_over_yml = discover(both.path());
        let nearest = discover(&ancestors_nested);

        // Then
        assert!(none.is_none());
        assert_eq!(from_ancestor, Some(root_cfg));
        // .yaml wins over .yml in the same directory, and the nearest
        // directory wins over an ancestor.
        assert_eq!(yaml_over_yml, Some(preferred));
        assert_eq!(nearest, Some(nested_cfg));
    }

    #[test]
    fn discover_ignores_a_directory_named_like_the_config() {
        // Given
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".lintmatter.yaml")).unwrap();

        // When
        let found = discover(dir.path());

        // Then
        assert!(found.is_none());
    }
}
