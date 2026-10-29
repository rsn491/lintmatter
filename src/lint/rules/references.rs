//! The file-reference rule: markdown links and path-shaped inline code spans
//! whose target looks like a local file but does not resolve to one.

use std::path::Path;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::lint::rule::{ALL_KINDS, Part, Rule, RuleMeta};
use crate::lint::{FileContext, FindingSink};
use crate::source::fence::FenceTracker;

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
    const META: RuleMeta = RuleMeta {
        id: "file-reference.missing",
        part: Part::FileReferences,
        kinds: ALL_KINDS,
    };

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
        for (i, line) in ctx.doc.body.split('\n').enumerate() {
            if !fences.scan_line(line) {
                continue;
            }
            for target in line_targets(line) {
                // Resolved lexically, so deciding whether a reference escapes
                // the tree costs no filesystem access of its own.
                let resolved = ctx.abs_path(&dir.join(&target));
                if !resolved.starts_with(&root) {
                    continue;
                }
                sink.applies();
                if std::fs::metadata(&resolved).is_err() {
                    sink.error(
                        offset + i + 1,
                        format!("referenced file {target:?} does not exist"),
                    );
                }
            }
        }
    }
}

/// The checkable file references on one line, deduplicated so a link and a
/// code span naming the same target report once.
fn line_targets(line: &str) -> Vec<String> {
    let masked = mask_code_spans(line);
    let mut targets: Vec<String> = Vec::new();
    for caps in LINK_RE.captures_iter(&masked) {
        if let Some(target) = link_target_path(&caps[1]) {
            targets.push(target);
        }
    }
    for caps in CODE_SPAN_RE.captures_iter(line) {
        if let Some(target) = code_span_target_path(&caps[1])
            && !targets.contains(&target)
        {
            targets.push(target);
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
    use crate::lint::testing::*;
    use std::fs;

    #[test]
    fn file_reference_rule() {
        {
            let dir = tempfile::tempdir().unwrap();
            write_file(dir.path(), "notes.md", "notes");
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), vec!["file-reference.missing"]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "See [docs](https://example.com/x), [mail](mailto:a@example.com) and [section](#heading).\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "Example syntax:\n\n```md\n[example](./missing.md)\n```\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
        {
            let (_base, target) = write_skill(
                "with-link",
                "---\nname: with-link\ndescription: A skill that references a missing helper script.\n---\nSee [helper](./helper.sh).\n",
            );
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), vec!["file-reference.missing"]);
        }
        {
            let dir = tempfile::tempdir().unwrap();
            let src = "# Instructions\n\nIntro line.\n\nSee [notes](./notes.md).\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert_eq!(res.findings.len(), 1);
            assert_eq!(res.findings[0].line, 5);
        }
    }

    #[test]
    fn file_references_outside_the_linted_tree_are_skipped() {
        // Escapes are skipped whether or not the target exists. lintmatter runs
        // in CI over checkouts it does not trust, and a finding that
        // distinguished "exists" from "does not exist" above the root would
        // make any file's markdown a probe for what is on the host.
        let outer = tempfile::tempdir().unwrap();
        write_file(outer.path(), "outside.md", "a real file above the root");
        let root = outer.path().join("root");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let escaping: &[(&str, &str)] = &[
            (
                "link to an existing file above the root",
                "See [x](../../outside.md).\n",
            ),
            (
                "link to a missing file above the root",
                "See [x](../../nowhere.md).\n",
            ),
            ("code span escaping the root", "Read `../../outside.md`.\n"),
        ];
        for (name, src) in escaping {
            let target = agents_target_under(&root, &sub, src);
            let res = lint(generous(), &target);
            assert!(
                res.findings.is_empty(),
                "{name}: {:?}",
                rule_ids(&res.findings)
            );
        }

        // Control: a missing reference that stays inside the tree is still
        // reported, so the cases above are not passing vacuously.
        let target = agents_target_under(&root, &sub, "See [x](./nowhere.md).\n");
        let res = lint(generous(), &target);
        assert_eq!(rule_ids(&res.findings), vec!["file-reference.missing"]);
    }

    #[test]
    fn file_reference_rule_nested_fences() {
        // A fence closes only on the same marker, at least as long, with no
        // info string. Each case pairs a source file with the rules it should
        // produce, so both directions of the old toggle bug are covered: the
        // block body must stay unscanned, and the tracker must not latch open
        // and swallow the prose that follows.
        let cases: &[(&str, &str, Vec<&str>)] = &[
            (
                "inner ``` does not close an outer ````",
                "````md\n```\n[x](./missing.md)\n```\nstill inside: [y](./gone.md)\n````\n",
                vec![],
            ),
            (
                "~~~ does not close a ``` block",
                "```\n~~~\n[x](./missing.md)\n~~~\n```\n",
                vec![],
            ),
            (
                "a fence carrying an info string does not close",
                "```\n```rust\n[x](./missing.md)\n```\n",
                vec![],
            ),
            (
                "prose after a closed block is still scanned",
                "````\n```\n````\n\nSee [y](./gone.md).\n",
                vec!["file-reference.missing"],
            ),
            (
                "a longer run closes a shorter block",
                "```\n[x](./missing.md)\n````\n\nSee [y](./gone.md).\n",
                vec!["file-reference.missing"],
            ),
            (
                "an indented closing fence still closes",
                "```\n[x](./missing.md)\n  ```\n\nSee [y](./gone.md).\n",
                vec!["file-reference.missing"],
            ),
        ];

        for (name, src, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), *want, "{name}");
        }
    }

    #[test]
    fn file_reference_rule_code_spans() {
        {
            // Path-shaped inline code span pointing at a missing file inside
            // the tree.
            let dir = tempfile::tempdir().unwrap();
            let sub = dir.path().join("sub");
            fs::create_dir_all(&sub).unwrap();
            let src = "Read instructions from `../planner_instructions.md`.\n";
            let target = agents_target_under(dir.path(), &sub, src);
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), vec!["file-reference.missing"]);
        }
        {
            // Same reference, but the file exists. A `../` hop that lands back
            // inside the linted tree must still be checked -- this is the
            // guard that skipping escapes did not swallow ordinary references.
            let dir = tempfile::tempdir().unwrap();
            let sub = dir.path().join("sub");
            fs::create_dir_all(&sub).unwrap();
            write_file(dir.path(), "planner_instructions.md", "notes");
            let src = "Read instructions from `../planner_instructions.md`.\n";
            let target = agents_target_under(dir.path(), &sub, src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
        {
            // Code spans that are not path-shaped are left alone.
            let dir = tempfile::tempdir().unwrap();
            let src = "Run `cargo test`, check `--strict`, or see `notes` and `./bin/lint`.\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
        {
            // A link and a code span pointing at the same missing target on
            // one line report once, not twice.
            let dir = tempfile::tempdir().unwrap();
            let src = "See [it](./missing.md) or `./missing.md`.\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), vec!["file-reference.missing"]);
        }
        {
            // Code spans inside fenced code blocks are still ignored.
            let dir = tempfile::tempdir().unwrap();
            let src = "Example:\n\n```md\nSee `../missing.md`.\n```\n";
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert!(res.findings.is_empty());
        }
    }

    #[test]
    fn file_reference_rule_links_inside_code_spans() {
        // Link syntax shown inside backticks is literal text, not a link, so
        // its target is not checked. A code span used as a link *label* is the
        // other direction of the same masking: that link is real and must
        // still be checked.
        let cases: &[(&str, &str, Vec<&str>)] = &[
            (
                "link syntax inside a code span is not a reference",
                "Write it as `[title](./missing.md)`.\n",
                vec![],
            ),
            (
                "an image inside a code span is not a reference",
                "Write it as `![alt](./missing.png)`.\n",
                vec![],
            ),
            (
                "a double-backtick span hides link syntax too",
                "Use ``[title](./missing.md)`` for that.\n",
                vec![],
            ),
            (
                "a code span in the label does not hide the link",
                "See [`notes`](./notes.md).\n",
                vec!["file-reference.missing"],
            ),
            (
                // The masked target is dropped rather than reported as a file
                // named `***`; the span is still read as a path on its own.
                "a masked span in the target position is not reported as `***`",
                "See [notes](`./notes.md`).\n",
                vec!["file-reference.missing"],
            ),
            (
                "masking one link leaves a real one on the same line",
                "See `[x](./missing.md)` and [y](./gone.md).\n",
                vec!["file-reference.missing"],
            ),
        ];

        for (name, src, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            let target = agents_target(dir.path(), src);
            let res = lint(generous(), &target);
            assert_eq!(rule_ids(&res.findings), *want, "{name}");
        }

        // The label case again with the file present: the link is checked, and
        // it resolves, so the case above is not passing on a parse failure.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "notes.md", "notes");
        let target = agents_target(dir.path(), "See [`notes`](./notes.md).\n");
        let res = lint(generous(), &target);
        assert!(res.findings.is_empty());
    }
}
