//! The two kinds of agent instruction file lintmatter understands.

use std::path::Path;

/// Distinguishes the two file types lintmatter understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Agents,
    Skill,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Agents => write!(f, "agents"),
            Kind::Skill => write!(f, "skill"),
        }
    }
}

/// Classifies a path by its base name, case-insensitively. Returns None when
/// the name is not an agent instruction file.
pub fn kind_for(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?.to_lowercase();
    match name.as_str() {
        "agents.md" => Some(Kind::Agents),
        "skill.md" => Some(Kind::Skill),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_for_matches_expected_names() {
        // Given
        let cases: &[(&str, Option<Kind>)] = &[
            ("AGENTS.md", Some(Kind::Agents)),
            ("agents.md", Some(Kind::Agents)),
            ("a/b/Agents.MD", Some(Kind::Agents)),
            ("SKILL.md", Some(Kind::Skill)),
            ("skill.md", Some(Kind::Skill)),
            ("SKILLS.md", None),
            ("README.md", None),
            ("AGENT.md", None),
            ("agents.markdown", None),
            ("AGENTS.md.bak", None),
        ];
        for (path, want) in cases {
            // When
            let got = kind_for(Path::new(path));

            // Then
            assert_eq!(got, *want, "{path}");
        }
    }
}
