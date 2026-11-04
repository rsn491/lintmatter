use crate::domain::Kind;

/// Marks how a finding affects the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
pub struct Finding {
    pub line: usize,
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

/// The measured token cost of a file's parts.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub content: usize,
    pub name: usize,
    pub description: usize,
}

/// Everything lintmatter learned about one file.
#[derive(Debug, Clone)]
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
