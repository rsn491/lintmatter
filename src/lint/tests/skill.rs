use super::*;
use crate::lint::*;

#[test]
fn skill_frontmatter_rules() {
    let long_name = "a".repeat(MAX_NAME_CHARS + 1);
    let long_desc = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
    let cases: Vec<(&str, &str, String, Vec<&str>)> = vec![
        ("valid skill has no findings", "valid-skill", VALID_SKILL.to_string(), vec![]),
        (
            "missing front matter",
            "no-frontmatter",
            "# A skill with no front matter\n\nBody.\n".to_string(),
            vec![RULE_FRONTMATTER_MISSING],
        ),
        (
            "front matter not first",
            "late-frontmatter",
            "\n---\nname: late-frontmatter\ndescription: Late.\n---\n".to_string(),
            vec![RULE_FRONTMATTER_NOT_FIRST],
        ),
        (
            "unterminated front matter",
            "unterminated",
            "---\nname: unterminated\ndescription: No closing fence.\n".to_string(),
            vec![RULE_FRONTMATTER_UNTERMINATED],
        ),
        (
            "invalid yaml",
            "bad-yaml",
            "---\nname: bad-yaml\n  description: wrong indent\n---\nBody.\n".to_string(),
            vec![RULE_FRONTMATTER_INVALID],
        ),
        (
            "front matter is a sequence",
            "sequence",
            "---\n- name: sequence\n---\nBody.\n".to_string(),
            vec![RULE_FRONTMATTER_INVALID],
        ),
        (
            "name missing",
            "nameless",
            "---\ndescription: A skill with no name.\n---\nBody.\n".to_string(),
            vec![RULE_NAME_REQUIRED],
        ),
        (
            "name empty",
            "empty-name",
            "---\nname: \"\"\ndescription: A skill with an empty name.\n---\nBody.\n".to_string(),
            vec![RULE_NAME_REQUIRED],
        ),
        (
            "name not a scalar",
            "listy-name",
            "---\nname:\n  - listy-name\ndescription: A skill whose name is a list.\n---\nBody.\n"
                .to_string(),
            vec![RULE_NAME_REQUIRED],
        ),
        (
            "name has bad characters",
            "Bad_Name",
            "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n".to_string(),
            vec![RULE_NAME_FORMAT],
        ),
        (
            "name has double hyphen",
            "double--hyphen",
            "---\nname: double--hyphen\ndescription: A skill with a doubled hyphen.\n---\nBody.\n"
                .to_string(),
            vec![RULE_NAME_FORMAT],
        ),
        (
            "name too long",
            "long",
            format!(
                "---\nname: {long_name}\ndescription: A skill with a very long name.\n---\nBody.\n"
            ),
            vec![RULE_NAME_LENGTH, RULE_NAME_DIR_MISMATCH],
        ),
        (
            "name does not match directory",
            "some-directory",
            "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n"
                .to_string(),
            vec![RULE_NAME_DIR_MISMATCH],
        ),
        (
            "description missing",
            "no-description",
            "---\nname: no-description\n---\nBody.\n".to_string(),
            vec![RULE_DESCRIPTION_REQUIRED],
        ),
        (
            "description empty",
            "blank-description",
            "---\nname: blank-description\ndescription: \"   \"\n---\nBody.\n".to_string(),
            vec![RULE_DESCRIPTION_REQUIRED],
        ),
        (
            "description too long",
            "verbose",
            format!("---\nname: verbose\ndescription: {long_desc}\n---\nBody.\n"),
            vec![RULE_DESCRIPTION_LENGTH],
        ),
        (
            "unknown key",
            "extra-keys",
            "---\nname: extra-keys\ndescription: A skill with a stray key.\nauthor: someone\n---\nBody.\n"
                .to_string(),
            vec![RULE_FRONTMATTER_UNKNOWN_KEY],
        ),
        (
            "known optional keys are accepted",
            "optional-keys",
            "---\nname: optional-keys\ndescription: A skill using every optional key.\nlicense: MIT\nallowed-tools:\n  - Read\n  - Bash\nmetadata:\n  owner: infra\n---\nBody.\n".to_string(),
            vec![],
        ),
        (
            "allowed-tools wrong type",
            "bad-tools",
            "---\nname: bad-tools\ndescription: A skill with a malformed tool list.\nallowed-tools:\n  read: true\n---\nBody.\n".to_string(),
            vec![RULE_ALLOWED_TOOLS_TYPE],
        ),
        (
            "metadata wrong type",
            "bad-metadata",
            "---\nname: bad-metadata\ndescription: A skill with scalar metadata.\nmetadata: not-a-mapping\n---\nBody.\n".to_string(),
            vec![RULE_METADATA_TYPE],
        ),
        (
            "several problems are all reported",
            "multi",
            "---\nname: Multi_Skill\nextra: true\n---\nBody.\n".to_string(),
            vec![
                RULE_FRONTMATTER_UNKNOWN_KEY,
                RULE_NAME_FORMAT,
                RULE_NAME_DIR_MISMATCH,
                RULE_DESCRIPTION_REQUIRED,
            ],
        ),
    ];

    for (name, skill_dir_name, src, want_rules) in cases {
        let (_base, target) = write_skill(skill_dir_name, &src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(rule_ids(&res.findings), want_rules, "{name}");
    }
}

#[test]
fn name_dir_mismatch_uses_injected_cwd() {
    // name.dir-mismatch deliberately ignores a skill sitting directly in
    // the working directory, since a loose SKILL.md is named for its
    // project rather than its folder. Injecting the cwd is what makes that
    // branch testable: reading it from the process would mean chdir-ing
    // while the rest of the suite runs concurrently.
    let (base, target) = write_skill(
        "some-directory",
        "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n",
    );
    let holding_dir = Path::new(&target.path).parent().unwrap().to_path_buf();

    let res = Linter::with_cwd(generous_config(), None, holding_dir)
        .file(&target)
        .unwrap();
    assert!(res.findings.is_empty(), "{:?}", rule_ids(&res.findings));

    let res = Linter::with_cwd(generous_config(), None, base.path().to_path_buf())
        .file(&target)
        .unwrap();
    assert_eq!(rule_ids(&res.findings), vec![RULE_NAME_DIR_MISMATCH]);
}

#[test]
fn strict_promotes_warnings() {
    let (_base, target) = write_skill(
        "mismatched-dir",
        "---\nname: other-name\ndescription: A skill in a mismatched directory.\n---\nBody.\n",
    );

    let cfg = generous_config();
    let res = Linter::new(cfg.clone(), None).file(&target).unwrap();
    assert_eq!(res.errors(), 0);
    assert_eq!(res.warnings(), 1);

    let mut cfg = cfg;
    cfg.strict = true;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(res.errors(), 1);
    assert_eq!(res.warnings(), 0);
}

#[test]
fn disable_skips_rules() {
    let (_base, target) = write_skill(
        "mismatched-dir",
        "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
    );

    let mut cfg = generous_config();
    cfg.disabled = vec![
        RULE_NAME_FORMAT.to_string(),
        RULE_NAME_DIR_MISMATCH.to_string(),
    ];
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert!(res.findings.is_empty());
}

#[test]
fn findings_carry_file_and_line() {
    let (_base, target) = write_skill(
        "lines",
        "---\nname: Bad_Name\ndescription: A skill with an invalid name.\n---\nBody.\n",
    );

    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    for f in &res.findings {
        assert_eq!(f.file, target.path);
        if f.rule == RULE_NAME_FORMAT {
            assert_eq!(f.line, 2);
        }
    }
}

#[test]
fn missing_file_is_an_error() {
    let base = tempfile::tempdir().unwrap();
    let target = Target {
        path: base.path().join("SKILL.md").to_string_lossy().to_string(),
        kind: Kind::Skill,
        root: base.path().to_path_buf(),
    };
    assert!(Linter::new(generous_config(), None).file(&target).is_err());
}
