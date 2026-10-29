use crate::discover::Kind;

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
    /// How well the file rates, 0 to 100.
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
