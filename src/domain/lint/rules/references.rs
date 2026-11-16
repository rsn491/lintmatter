//! The file-reference rule: markdown links and path-shaped inline code spans
//! whose target looks like a local file but does not resolve to one.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::domain::Kind;
use crate::domain::fence::FenceTracker;
use crate::domain::lint::rule::Rule;
use crate::domain::lint::{FileContext, FindingSink, RULE_FILE_REFERENCE_MISSING};
use crate::utils::wildcard_match;

/// Matches inline markdown links and images: `[text](target)` or
/// `![alt](target)`. Does not handle reference-style links (`[text][ref]`).
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[[^\]]*\]\(([^)]+)\)").expect("valid regex literal"));

/// Matches inline code spans: `` `text` ``. Does double duty: a span's contents
/// are masked out of the link scan, and are then read as a reference in their
/// own right. Fenced code blocks are excluded separately, by the FenceTracker.
static CODE_SPAN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`]+)`").expect("valid regex literal"));

/// Detects an absolute URI (`https://`, `mailto:`, `tel:`, and the like) so
/// those targets are left for a browser rather than checked as files.
static URI_SCHEME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").expect("valid regex literal"));

/// Applies to both AGENTS.md and SKILL.md, since a dangling reference breaks
/// the file's contract with the runtime regardless of kind.
///
/// Tuned against false positives seen in public skill repositories: quoted
/// lines, placeholder link targets and `ignore-references` globs are skipped,
/// and a miss in inline code only warns (see [`Source`]).
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
        let path = &ctx.target.path;
        let dir = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
        let root = ctx.abs_path(&ctx.target.root);
        let offset = if ctx.doc.frontmatter.present {
            ctx.doc.frontmatter.end_line
        } else {
            0
        };

        let mut fences = FenceTracker::default();
        ctx.doc
            .body
            .split('\n')
            .enumerate()
            .filter(|(_, line)| fences.scan_line(line) && !is_blockquote(line))
            .flat_map(|(i, line)| line_targets(line).into_iter().map(move |t| (i, t)))
            .filter(|(_, reference)| !is_ignored(&reference.target, &ctx.cfg.ignore_references))
            .for_each(|(i, reference)| {
                // Resolved lexically, so deciding whether a reference escapes
                // the tree costs no filesystem access of its own.
                let candidates: Vec<PathBuf> = reference
                    .bases(dir, &root)
                    .into_iter()
                    .map(|base| ctx.abs_path(&base.join(&reference.target)))
                    .filter(|resolved| resolved.starts_with(&root))
                    .collect();
                if candidates.is_empty() {
                    return;
                }
                sink.applies();
                if candidates.iter().any(|c| ctx.exists(&root, c)) {
                    return;
                }
                let line = offset + i + 1;
                let msg = format!("referenced file {:?} does not exist", reference.target);
                match reference.source {
                    Source::Link => sink.error(line, msg),
                    Source::CodeSpan => sink.warn(line, msg),
                }
            });
    }
}

/// Where on the line a reference was written. A markdown link promises a file
/// beside the document, so a dangling one is an error. A path in inline code
/// is only probably a reference -- prose also uses the form for example paths
/// and for files in the reader's own project -- so a miss is a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Link,
    CodeSpan,
}

#[derive(Debug, PartialEq, Eq)]
struct Reference {
    target: String,
    source: Source,
}

impl Reference {
    /// The directories the target may be relative to. A link resolves the way
    /// a markdown renderer would, against the document's own directory. A code
    /// span is as often written from the top of the tree, so either counts.
    fn bases<'a>(&self, dir: &'a Path, root: &'a Path) -> Vec<&'a Path> {
        match self.source {
            Source::Link => vec![dir],
            Source::CodeSpan => vec![dir, root],
        }
    }
}

/// Quoted text is someone else's words -- a reply template, a pasted excerpt
/// -- so its links are relative to wherever the quote ends up, not this file.
fn is_blockquote(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

/// Whether the project's `ignore-references` globs cover this target. A
/// leading `./` is dropped from both sides so `run/*` and `./run/*` mean the
/// same thing.
fn is_ignored(target: &str, patterns: &[String]) -> bool {
    let bare = |s: &'_ str| -> String { s.strip_prefix("./").unwrap_or(s).to_string() };
    let target = bare(target);
    patterns
        .iter()
        .any(|pattern| wildcard_match(&bare(pattern), &target))
}

/// The checkable file references on one line, deduplicated so a link and a
/// code span naming the same target report once, as the link.
fn line_targets(line: &str) -> Vec<Reference> {
    let masked = mask_code_spans(line);
    let links: Vec<Reference> = LINK_RE
        .captures_iter(&masked)
        .filter_map(|caps| link_target_path(&caps[1]))
        .map(|target| Reference {
            target,
            source: Source::Link,
        })
        .collect();
    CODE_SPAN_RE
        .captures_iter(line)
        .filter_map(|caps| code_span_target_path(&caps[1]))
        .fold(links, |mut targets, target| {
            if !targets.iter().any(|r| r.target == target) {
                targets.push(Reference {
                    target,
                    source: Source::CodeSpan,
                });
            }
            targets
        })
}

/// Blanks out inline code spans so link syntax shown inside backticks --
/// `` `[title](./example.md)` `` -- reads as the literal text it is rather than
/// as a reference. Masking rather than deleting keeps a code span used as a
/// link label, ``[`notes`](./notes.md)``, a link whose target is still checked.
///
/// The filler is `*` because `link_target_path` already rejects it: a masked
/// span sitting in a link's target position is then dropped, not reported as a
/// missing file named `****`.
fn mask_code_spans(line: &str) -> String {
    CODE_SPAN_RE
        .replace_all(line, |caps: &Captures<'_>| {
            "*".repeat(caps[0].chars().count())
        })
        .into_owned()
}

/// Extracts the file path a markdown link points at, or `None` when the link
/// is not a checkable local file reference: an absolute URL, an in-page
/// anchor, an absolute path, or a placeholder (templated, or a bare word).
fn link_target_path(raw: &str) -> Option<String> {
    let mut target = raw.trim().to_string();
    if let Some(rest) = target.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        target = rest[..end].to_string();
        // `<ticket url>` is a slot to fill in; a real angle-bracket target
        // with a space in it is rare enough to give up.
        if target.contains([' ', '\t']) {
            return None;
        }
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
    // A bare word such as `URL` or `link` is a placeholder far more often than
    // an extensionless file sitting beside the document.
    if !target.contains(['/', '.']) {
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
