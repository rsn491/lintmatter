use crate::discover::Kind;
use crate::lint::{Counts, FileResult, Finding, Severity};

pub(super) fn results() -> Vec<FileResult> {
    vec![
        FileResult {
            path: "AGENTS.md".to_string(),
            kind: Kind::Agents,
            tokens: Counts {
                content: 6142,
                name: 0,
                description: 0,
            },
            score: 50,
            findings: vec![Finding {
                line: 0,
                rule: crate::lint::RULE_TOKENS_CONTENT,
                severity: Severity::Error,
                message: "content is 6,142 tokens, over the 5,000 token limit".to_string(),
            }],
        },
        FileResult {
            path: "skills/thing/SKILL.md".to_string(),
            kind: Kind::Skill,
            tokens: Counts {
                content: 120,
                name: 2,
                description: 30,
            },
            score: 96,
            findings: vec![Finding {
                line: 2,
                rule: crate::lint::RULE_NAME_DIR_MISMATCH,
                severity: Severity::Warning,
                message: "name \"other\" does not match its directory \"thing\"".to_string(),
            }],
        },
    ]
}
