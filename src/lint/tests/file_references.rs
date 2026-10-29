use super::*;
use crate::lint::*;

#[test]
fn file_reference_rule() {
    {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "notes.md", "notes");
        let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
    {
        let dir = tempfile::tempdir().unwrap();
        let src = "# Instructions\n\nSee [notes](./notes.md) for details.\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
    }
    {
        let dir = tempfile::tempdir().unwrap();
        let src = "See [docs](https://example.com/x), [mail](mailto:a@example.com) and [section](#heading).\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
    {
        let dir = tempfile::tempdir().unwrap();
        let src = "Example syntax:\n\n```md\n[example](./missing.md)\n```\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
    {
        let (_base, target) = write_skill(
            "with-link",
            "---\nname: with-link\ndescription: A skill that references a missing helper script.\n---\nSee [helper](./helper.sh).\n",
        );
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
    }
    {
        let dir = tempfile::tempdir().unwrap();
        let src = "# Instructions\n\nIntro line.\n\nSee [notes](./notes.md).\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(
            res.findings.is_empty(),
            "{name}: {:?}",
            rule_ids(&res.findings)
        );
    }

    // Control: a missing reference that stays inside the tree is still
    // reported, so the cases above are not passing vacuously.
    let target = agents_target_under(&root, &sub, "See [x](./nowhere.md).\n");
    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
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
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
        (
            "a longer run closes a shorter block",
            "```\n[x](./missing.md)\n````\n\nSee [y](./gone.md).\n",
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
        (
            "an indented closing fence still closes",
            "```\n[x](./missing.md)\n  ```\n\nSee [y](./gone.md).\n",
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
    ];

    for (name, src, want) in cases {
        let dir = tempfile::tempdir().unwrap();
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
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
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
    {
        // Code spans that are not path-shaped are left alone.
        let dir = tempfile::tempdir().unwrap();
        let src = "Run `cargo test`, check `--strict`, or see `notes` and `./bin/lint`.\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert!(res.findings.is_empty());
    }
    {
        // A link and a code span pointing at the same missing target on
        // one line report once, not twice.
        let dir = tempfile::tempdir().unwrap();
        let src = "See [it](./missing.md) or `./missing.md`.\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), vec![RULE_FILE_REFERENCE_MISSING]);
    }
    {
        // Code spans inside fenced code blocks are still ignored.
        let dir = tempfile::tempdir().unwrap();
        let src = "Example:\n\n```md\nSee `../missing.md`.\n```\n";
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
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
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
        (
            // The masked target is dropped rather than reported as a file
            // named `***`; the span is still read as a path on its own.
            "a masked span in the target position is not reported as `***`",
            "See [notes](`./notes.md`).\n",
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
        (
            "masking one link leaves a real one on the same line",
            "See `[x](./missing.md)` and [y](./gone.md).\n",
            vec![RULE_FILE_REFERENCE_MISSING],
        ),
    ];

    for (name, src, want) in cases {
        let dir = tempfile::tempdir().unwrap();
        let target = agents_target(dir.path(), src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), *want, "{name}");
    }

    // The label case again with the file present: the link is checked, and
    // it resolves, so the case above is not passing on a parse failure.
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "notes.md", "notes");
    let target = agents_target(dir.path(), "See [`notes`](./notes.md).\n");
    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert!(res.findings.is_empty());
}
