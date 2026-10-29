//! Runs the registered rules over a file.

use std::collections::HashSet;
use std::path::PathBuf;

use super::context::FileContext;
use super::finding::{Counts, FileResult};
use super::rules;
use super::score::{Tally, score};
use super::sink::FindingSink;
use crate::config::Config;
use crate::discover::{Kind, Target};
use crate::parse::Document;
use crate::tokens::{self, Counter};

/// Applies [`Config`]'s rules using a token [`Counter`].
pub struct Linter {
    cfg: Config,
    counter: Box<dyn Counter>,
    disabled: HashSet<String>,
    /// Resolved once, at construction. Rules read this through FileContext
    /// instead of calling `std::env::current_dir` themselves.
    cwd: PathBuf,
}

impl Linter {
    /// Returns a Linter anchored to the process's working directory. `None`
    /// for `counter` falls back to the heuristic estimator.
    pub fn new(cfg: Config, counter: Option<Box<dyn Counter>>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self::with_cwd(cfg, counter, cwd)
    }

    /// Returns a Linter that treats `cwd` as the working directory, so the one
    /// rule that cares can be tested without mutating process-global state
    /// while other tests run alongside it.
    pub fn with_cwd(cfg: Config, counter: Option<Box<dyn Counter>>, cwd: PathBuf) -> Self {
        let counter = counter.unwrap_or_else(|| Box::new(tokens::Estimator::new()));
        let disabled = cfg.disabled.iter().cloned().collect();
        Linter {
            cfg,
            counter,
            disabled,
            cwd,
        }
    }

    /// Reads and lints one target.
    pub fn file(&self, t: &Target) -> Result<FileResult, String> {
        let src = std::fs::read(&t.path).map_err(|e| format!("cannot read {}: {e}", t.path))?;
        Ok(self.check(t, &src))
    }

    /// Lints already-read file contents.
    ///
    /// Rules run in registry order, so findings come out in that order without
    /// a sort: output never depends on the order checks happen to run in.
    pub fn check(&self, t: &Target, src: &[u8]) -> FileResult {
        let doc = Document::parse(src);

        // Only a skill has a name and description to be judged, so only a
        // skill carries those token counts. Measured once here rather than as
        // a side effect of validating them, which is what let the rules stop
        // passing a mutable Counts around.
        let fm = if t.kind == Kind::Skill {
            doc.mapping()
        } else {
            None
        };
        let name = fm.and_then(|f| non_empty(f.string("name")));
        let description = fm.and_then(|f| non_empty(f.string("description")));
        let tokens = Counts {
            content: self.counter.count(&doc.body),
            name: name.as_deref().map_or(0, |s| self.counter.count(s)),
            description: description.as_deref().map_or(0, |s| self.counter.count(s)),
        };

        let mut findings = Vec::new();
        let mut tally = Tally::default();
        {
            let ctx = FileContext::new(t, &doc, &tokens, &self.cfg, &self.cwd, name, description);
            let mut sink = FindingSink::new(&mut findings, &mut tally, self.cfg.strict, &t.path);
            for rule in rules::all() {
                if !rule.applies_to(t.kind) || self.disabled.contains(rule.id()) {
                    continue;
                }
                sink.set_rule(rule.id());
                rule.check(&ctx, &mut sink);
            }
        }

        FileResult {
            path: t.path.clone(),
            kind: t.kind,
            score: score(&tokens, &self.cfg, t.kind, &tally),
            tokens,
            findings,
        }
    }
}

/// Trims a front-matter scalar, discarding it when nothing is left.
fn non_empty(raw: Option<String>) -> Option<String> {
    let s = raw?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
