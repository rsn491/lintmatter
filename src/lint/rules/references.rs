//! The file-reference rule: markdown links and path-shaped inline code spans
//! whose target looks like a local file but does not resolve to one.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::discover::Kind;
use crate::fence::FenceTracker;
use crate::lint::rule::Rule;
use crate::lint::{FileContext, FindingSink, RULE_FILE_REFERENCE_MISSING};
use crate::parse::Document;

/// Matches inline markdown links and images: `[text](target)` or
/// `![alt](target)`. Does not handle reference-style links (`[text][ref]`).
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!?\[[^\]]*\]\(([^)]+)\)").unwrap());

/// Matches inline code spans: `` `text` ``. Does double duty: a span's contents
/// are masked out of the link scan, and are then read as a reference in their
/// own right. Fenced code blocks are excluded separately, by the FenceTracker.
static CODE_SPAN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").unwrap());

/// Detects an absolute URI (`https://`, `mailto:`, `tel:`, and the like) so
/// those targets are left for a browser rather than checked as files.
static URI_SCHEME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").unwrap());

/// Applies to both AGENTS.md and SKILL.md, since a dangling reference breaks
/// the file's contract with the runtime regardless of kind.
///
/// References resolving outside the linted tree are left alone, like URLs and
/// absolute paths: lintmatter often runs in CI over a checkout it does not trust,
/// and stat-ing whatever path a file names would turn its markdown into a
/// probe for what exists on the host.
pub struct Missing;

impl Rule for Missing {
    fn id(&self) -> &'static str {
        RULE_FILE_REFERENCE_MISSING
    }

    fn applies_to(&self, _kind: Kind) -> bool {
        true
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        let root = ctx.abs_path(&ctx.target.root);
        for r in scan(ctx.doc, &ctx.target.path, &root, |p| ctx.abs_path(p)) {
            sink.applies();
            if r.missing {
                sink.error(
                    r.line,
                    format!("referenced file {:?} does not exist", r.target),
                );
            }
        }
    }
}

/// One checkable reference inside the linted tree.
pub(crate) struct Reference {
    /// The 1-based line in the file, front matter included.
    pub line: usize,
    pub target: String,
    /// Where `target` sits on its line, as byte ranges into the body line:
    /// one per place it was written, since a link and a code span naming the
    /// same file report once but both have to be rewritten by `--fix`.
    pub spans: Vec<Range<usize>>,
    pub missing: bool,
}

/// Every checkable reference in `doc`'s body that resolves inside `root`.
/// Shared by the rule and by `--fix`, so the two cannot disagree about what
/// counts as a reference. `abs` makes a path absolute and lexically clean.
pub(crate) fn scan(
    doc: &Document,
    path: &str,
    root: &Path,
    abs: impl Fn(&Path) -> PathBuf,
) -> Vec<Reference> {
    let dir = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
    let offset = if doc.frontmatter.present {
        doc.frontmatter.end_line
    } else {
        0
    };

    let mut refs = Vec::new();
    let mut fences = FenceTracker::default();
    for (i, line) in doc.body.split('\n').enumerate() {
        if !fences.scan_line(line) {
            continue;
        }
        for (target, spans) in line_targets(line) {
            // Resolved lexically, so deciding whether a reference escapes
            // the tree costs no filesystem access of its own.
            let resolved = abs(&dir.join(&target));
            if !resolved.starts_with(root) {
                continue;
            }
            refs.push(Reference {
                line: offset + i + 1,
                missing: std::fs::metadata(&resolved).is_err(),
                target,
                spans,
            });
        }
    }
    refs
}

/// The checkable file references on one line with where each was written,
/// deduplicated so a link and a code span naming the same target report once.
fn line_targets(line: &str) -> Vec<(String, Vec<Range<usize>>)> {
    let masked = mask_code_spans(line);
    let mut targets: Vec<(String, Vec<Range<usize>>)> = Vec::new();
    let mut push = |target: String, start: usize| {
        let span = start..start + target.len();
        match targets.iter_mut().find(|(t, _)| *t == target) {
            Some((_, spans)) => spans.push(span),
            None => targets.push((target, vec![span])),
        }
    };
    for caps in LINK_RE.captures_iter(&masked) {
        let raw = caps.get(1).unwrap();
        if let Some(target) = link_target_path(raw.as_str()) {
            // The path is a prefix of the target once whitespace and any `<`
            // are stripped, so its first occurrence is where it was written.
            let at = raw.as_str().find(&target).unwrap_or(0);
            push(target, raw.start() + at);
        }
    }
    for caps in CODE_SPAN_RE.captures_iter(line) {
        let raw = caps.get(1).unwrap();
        if let Some(target) = code_span_target_path(raw.as_str()) {
            let lead = raw.as_str().len() - raw.as_str().trim_start().len();
            push(target, raw.start() + lead);
        }
    }
    targets
}

/// Blanks out inline code spans so link syntax shown inside backticks --
/// `` `[title](./example.md)` `` -- reads as the literal text it is rather than
/// as a reference. Masking rather than deleting keeps a code span used as a
/// link label, ``[`notes`](./notes.md)``, a link whose target is still checked.
///
/// The filler is `*` because `link_target_path` already rejects it: a masked
/// span sitting in a link's target position is then dropped, not reported as a
/// missing file named `****`. One filler per byte, not per character, so byte
/// offsets into the masked line still point into the original.
fn mask_code_spans(line: &str) -> String {
    CODE_SPAN_RE
        .replace_all(line, |caps: &Captures<'_>| "*".repeat(caps[0].len()))
        .into_owned()
}

/// Extracts the file path a markdown link points at, or `None` when the link
/// is not a checkable local file reference: an absolute URL, an in-page
/// anchor, an absolute path, or a templated placeholder.
fn link_target_path(raw: &str) -> Option<String> {
    let mut target = raw.trim().to_string();
    if let Some(rest) = target.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        target = rest[..end].to_string();
    }
    if let Some(sp) = target.find([' ', '\t']) {
        target.truncate(sp);
    }
    if target.is_empty() || target.starts_with('#') {
        return None;
    }
    if URI_SCHEME_RE.is_match(&target) || target.starts_with("//") {
        return None;
    }
    if let Some(frag) = target.find(['#', '?']) {
        target.truncate(frag);
    }
    if target.is_empty()
        || Path::new(&target).is_absolute()
        || target.contains(['{', '}', '$', '*'])
    {
        return None;
    }
    Some(target)
}

/// Extracts a checkable file path from an inline code span, or `None` when
/// the span is not clearly a relative file reference. Narrowly scoped to
/// avoid false positives on flags, identifiers, and shell snippets: the span
/// must be a single whitespace-free token starting with `./` or `../` and
/// ending in a file extension.
fn code_span_target_path(raw: &str) -> Option<String> {
    let target = raw.trim();
    if target.is_empty() || target.chars().any(char::is_whitespace) {
        return None;
    }
    if !(target.starts_with("./") || target.starts_with("../")) {
        return None;
    }
    if target.contains(['{', '}', '$', '*', '#', '?']) {
        return None;
    }
    let last_seg = target.rsplit('/').next().unwrap_or(target);
    let (_, ext) = last_seg.rsplit_once('.')?;
    if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(target.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_targets_locate_each_reference() {
        let line = "See [`a`](<./a.md> \"t\"), ` ./b.md ` and [c](./a.md#x).";
        let found: Vec<(String, Vec<&str>)> = line_targets(line)
            .into_iter()
            .map(|(t, spans)| (t, spans.into_iter().map(|s| &line[s]).collect()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("./a.md".to_string(), vec!["./a.md", "./a.md"]),
                ("./b.md".to_string(), vec!["./b.md"]),
            ]
        );
    }
}
