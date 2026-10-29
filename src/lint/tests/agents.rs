use super::*;
use crate::lint::*;

#[test]
fn agents_frontmatter_is_not_validated() {
    let base = tempfile::tempdir().unwrap();
    let src = "---\ntitle: Project instructions\nowner: infra\n---\n\n# Instructions\n\nBody.\n";
    let target = agents_target(base.path(), src);

    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert!(res.findings.is_empty());
    assert!(res.tokens.content > 0);
}

#[test]
fn agents_unterminated_frontmatter_is_reported() {
    let base = tempfile::tempdir().unwrap();
    let src = "---\ntitle: Project instructions\n\n# Instructions never closed\n";
    let target = agents_target(base.path(), src);

    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(rule_ids(&res.findings), vec![RULE_FRONTMATTER_UNTERMINATED]);
}

#[test]
fn agents_frontmatter_excluded_from_content() {
    let base = tempfile::tempdir().unwrap();
    let fm_src =
        "---\ntitle: A fairly wordy front matter block that costs real tokens\n---\nBody.\n";
    let with_fm = agents_target(base.path(), fm_src);
    let base2 = tempfile::tempdir().unwrap();
    let bare = agents_target(base2.path(), "Body.\n");

    let linter = Linter::new(generous_config(), None);
    let with_res = linter.file(&with_fm).unwrap();
    let bare_res = linter.file(&bare).unwrap();
    assert_eq!(with_res.tokens.content, bare_res.tokens.content);
}
