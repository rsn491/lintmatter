//! The domain types every layer shares: what a file to lint is, what a
//! finding is, and what a run learned about a file. Nothing here depends on
//! another lintmatter module, so every other module may depend on it.

mod budgets;

pub use budgets::{
    Budgets, DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
    DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS, FIELDS, Field,
};

use std::path::PathBuf;

/// Distinguishes the two file types lintmatter understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
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

/// A single file to lint.
#[derive(Debug, Clone)]
pub struct Target {
    pub path: String,
    pub kind: Kind,
    /// The tree this target was found under: the directory argument that
    /// produced it, or the file's own directory when named explicitly. Rules
    /// use it to tell a reference into the linted tree from one that escapes.
    pub root: PathBuf,
}

/// Marks how a finding affects the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// A single rule violation.
///
/// The file is the enclosing [`FileResult`]'s path; the JSON reporter re-adds it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Finding {
    #[serde(skip_serializing_if = "is_zero")]
    pub line: usize,
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The measured token cost of a file's parts.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Counts {
    pub content: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub name: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub description: usize,
}

/// Everything lintmatter learned about one file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileResult {
    pub path: String,
    pub kind: Kind,
    pub tokens: Counts,
    /// How well the file rates, 0 to 100. See [`score`].
    pub score: u8,
    pub findings: Vec<Finding>,
}

impl FileResult {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == s).count()
    }
}
