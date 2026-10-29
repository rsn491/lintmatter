//! `--fix`: repoints broken file references at a file of the same name.
//!
//! A reference usually breaks because the file it names moved, not because it
//! was deleted, so the likeliest intended target is a file with exactly that
//! name somewhere else in the tree. Of those, the one closest to the
//! referencing file wins: skills keep their helpers next to them, so distance
//! is the best tie-breaker there is. When two candidates are equally close
//! nothing is guessed; the reference is left for a person, and the lint that
//! follows still reports it.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::discover::{MAX_DEPTH, SKIP_DIRS, Target};
use crate::lint::rules::references;
use crate::parse::Document;
use crate::utils::clean_path;

const BOM: &str = "\u{feff}";

/// What `--fix` did with one broken reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The reference on `line` was rewritten from `from` to `to`.
    Fixed {
        line: usize,
        from: String,
        to: String,
    },
    /// More than one file of the right name sat at the same, closest distance.
    Ambiguous {
        line: usize,
        target: String,
        candidates: usize,
    },
}

/// Rewrites broken references, file by file, reusing one index of the tree's
/// files per walk root.
pub struct Fixer {
    cwd: PathBuf,
    /// Walk root to file name to every file of that name under it.
    index: HashMap<PathBuf, HashMap<String, Vec<PathBuf>>>,
}

impl Fixer {
    pub fn new(cwd: PathBuf) -> Self {
        Fixer {
            cwd,
            index: HashMap::new(),
        }
    }

    /// Fixes what it can in one target and writes the file back if anything
    /// changed. Errs only when that write fails. Only references the `file-reference.missing` rule would report
    /// are touched, and candidates come from the same tree that rule confines
    /// itself to, so `--fix` never reaches outside what is being linted.
    pub fn file(&mut self, t: &Target) -> Result<Vec<Outcome>, String> {
        // An unreadable file is left for the lint pass, which already reports
        // it; saying so twice would only add noise.
        let Ok(raw) = fs::read(&t.path) else {
            return Ok(Vec::new());
        };
        // A file that is not UTF-8 is linted lossily, so its byte offsets do
        // not line up with the source: leave it alone rather than corrupt it.
        let Ok(src) = std::str::from_utf8(&raw) else {
            return Ok(Vec::new());
        };
        let doc = Document::parse(src.as_bytes());
        let root = self.abs(&t.root);
        let refs = references::scan(&doc, &t.path, &root, |p| self.abs(p));
        if !refs.iter().any(|r| r.missing) {
            return Ok(Vec::new());
        }

        let from_dir = self
            .abs(Path::new(&t.path))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let by_name = self
            .index
            .entry(root.clone())
            .or_insert_with(|| index_files(&root));

        // The parser strips a BOM and keeps everything else, `\r` included, so
        // body lines are the source's lines byte for byte once it is gone.
        let (bom, text) = match src.strip_prefix(BOM) {
            Some(rest) => (BOM, rest),
            None => ("", src),
        };
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let mut outcomes = Vec::new();
        let mut edits = Vec::new();
        for r in refs.iter().filter(|r| r.missing) {
            let name = r.target.rsplit('/').next().unwrap_or(&r.target);
            let found = by_name.get(name).map(Vec::as_slice).unwrap_or_default();
            let best = match closest(&from_dir, found) {
                Closest::None => continue,
                Closest::Tied(n) => {
                    outcomes.push(Outcome::Ambiguous {
                        line: r.line,
                        target: r.target.clone(),
                        candidates: n,
                    });
                    continue;
                }
                Closest::One(best) => best,
            };
            let mut to = relative(&from_dir, best);
            // A code span is only read as a reference when it starts with `./`
            // or `../`, so keep the prefix whenever the original had one.
            if r.target.starts_with("./") && !to.starts_with("../") {
                to = format!("./{to}");
            }
            let Some(line) = lines.get(r.line - 1) else {
                continue;
            };
            if r.spans
                .iter()
                .any(|s| line.get(s.clone()) != Some(&r.target))
            {
                continue;
            }
            edits.extend(r.spans.iter().map(|s| (r.line - 1, s.clone(), to.clone())));
            outcomes.push(Outcome::Fixed {
                line: r.line,
                from: r.target.clone(),
                to,
            });
        }

        // Applied right to left, so an edit never shifts the offsets of one
        // still to come on the same line.
        edits.sort_by_key(|(line, span, _)| std::cmp::Reverse((*line, span.start)));
        for (line, span, to) in &edits {
            lines[*line].replace_range(span.clone(), to);
        }
        if !edits.is_empty() {
            let fixed = format!("{bom}{}", lines.join("\n"));
            fs::write(&t.path, fixed).map_err(|e| format!("cannot write {}: {e}", t.path))?;
        }
        Ok(outcomes)
    }

    fn abs(&self, path: &Path) -> PathBuf {
        clean_path(&self.cwd.join(path))
    }
}

enum Closest<'a> {
    None,
    One(&'a Path),
    Tied(usize),
}

/// The candidate fewest directory hops from `dir`, counting the climb to the
/// common ancestor and the descent from it.
fn closest<'a>(dir: &Path, candidates: &'a [PathBuf]) -> Closest<'a> {
    let mut best: Vec<&Path> = Vec::new();
    let mut best_hops = usize::MAX;
    for c in candidates {
        let hops = hops(dir, c.parent().unwrap_or(c));
        if hops < best_hops {
            best_hops = hops;
            best.clear();
        }
        if hops == best_hops {
            best.push(c);
        }
    }
    match best.as_slice() {
        [] => Closest::None,
        [one] => Closest::One(one),
        many => Closest::Tied(many.len()),
    }
}

fn hops(from: &Path, to: &Path) -> usize {
    let common = common_len(from, to);
    from.components().count() - common + to.components().count() - common
}

fn common_len(a: &Path, b: &Path) -> usize {
    a.components()
        .zip(b.components())
        .take_while(|(x, y)| x == y)
        .count()
}

/// `to` relative to the directory `from`, with forward slashes.
fn relative(from: &Path, to: &Path) -> String {
    let common = common_len(from, to);
    let ups = from.components().count() - common;
    let mut parts: Vec<String> = vec!["..".to_string(); ups];
    parts.extend(to.components().skip(common).filter_map(|c| match c {
        Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
        _ => None,
    }));
    parts.join("/")
}

/// Every regular file under `root` by name. Prunes the directories discovery
/// prunes and follows no symlinks, so a candidate is always a file that is
/// really inside the tree.
fn index_files(root: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if file_type.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((entry.path(), depth + 1));
                }
            } else if file_type.is_file() {
                index.entry(name).or_default().push(entry.path());
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Kind;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, contents) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    /// Runs the fixer over `rel` under `dir`, returning what it reported and
    /// the file's contents afterwards.
    fn fix(dir: &Path, rel: &str) -> (Vec<Outcome>, String) {
        let path = dir.join(rel);
        let target = Target {
            path: path.to_string_lossy().to_string(),
            kind: Kind::Skill,
            root: dir.to_path_buf(),
        };
        let outcomes = Fixer::new(PathBuf::new()).file(&target).unwrap();
        (outcomes, fs::read_to_string(path).unwrap())
    }

    const SKILL: &str = "skills/a/SKILL.md";

    #[test]
    fn picks_the_closest_file_of_the_same_name() {
        let dir = tree(&[
            (SKILL, "---\nname: a\n---\nRun [it](./helper.sh).\n"),
            ("skills/a/scripts/helper.sh", ""),
            ("tools/deep/er/helper.sh", ""),
        ]);
        let (outcomes, src) = fix(dir.path(), SKILL);
        assert_eq!(src, "---\nname: a\n---\nRun [it](./scripts/helper.sh).\n");
        assert_eq!(
            outcomes,
            vec![Outcome::Fixed {
                line: 4,
                from: "./helper.sh".to_string(),
                to: "./scripts/helper.sh".to_string(),
            }]
        );
    }

    #[test]
    fn climbs_out_of_the_skill_when_that_is_closest() {
        let dir = tree(&[
            (SKILL, "See [x](notes.md#usage) and `./notes.md`.\n"),
            ("skills/shared/notes.md", ""),
        ]);
        let (_, src) = fix(dir.path(), SKILL);
        assert_eq!(
            src,
            "See [x](../shared/notes.md#usage) and `../shared/notes.md`.\n"
        );
    }

    #[test]
    fn equally_close_candidates_are_left_alone() {
        let original = "Run [it](./helper.sh).\n";
        let dir = tree(&[
            (SKILL, original),
            ("skills/a/x/helper.sh", ""),
            ("skills/a/y/helper.sh", ""),
        ]);
        let (outcomes, src) = fix(dir.path(), SKILL);
        assert_eq!(src, original);
        assert_eq!(
            outcomes,
            vec![Outcome::Ambiguous {
                line: 1,
                target: "./helper.sh".to_string(),
                candidates: 2,
            }]
        );
    }

    #[test]
    fn leaves_what_the_rule_does_not_report() {
        let original = "```\n[x](./helper.sh)\n```\n[y](../../../helper.sh) [z](./Helper.sh) [ok](./here.md)\n";
        let dir = tree(&[
            (SKILL, original),
            ("skills/a/here.md", ""),
            ("skills/a/scripts/helper.sh", ""),
            ("node_modules/pkg/Helper.sh", ""),
        ]);
        // Fenced, escaping the tree, a name matched only case-insensitively
        // or only inside a pruned directory, and a reference that resolves.
        let (outcomes, src) = fix(dir.path(), SKILL);
        assert_eq!(src, original);
        assert!(outcomes.is_empty(), "{outcomes:?}");
    }

    #[test]
    fn keeps_a_bom_and_crlf_line_endings() {
        let dir = tree(&[
            (SKILL, "\u{feff}Run [it](./helper.sh).\r\nDone.\r\n"),
            ("skills/a/bin/helper.sh", ""),
        ]);
        let (_, src) = fix(dir.path(), SKILL);
        assert_eq!(src, "\u{feff}Run [it](./bin/helper.sh).\r\nDone.\r\n");
    }
}
