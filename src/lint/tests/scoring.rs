use super::*;
use crate::lint::score::part;
use crate::lint::*;

/// Guards the score against a rule being added and rated by accident: a
/// new id has to be given a part before this passes.
#[test]
fn every_rule_belongs_to_a_score_part() {
    for rule in RULES.iter() {
        assert!(part(rule).is_some(), "{rule} is not assigned a score part");
    }
}

#[test]
fn frontmatter_score() {
    let valid_body = "description: Use this skill when you need a fixture that passes.";
    let cases: Vec<(&str, &str, String, u8)> = vec![
        (
            "clean skill passes every rule applied to it",
            "valid-skill",
            VALID_SKILL.to_string(),
            100,
        ),
        (
            "one of eight rules fails, and 87.5 floors to 87",
            "Bad_Name",
            format!("---\nname: Bad_Name\n{valid_body}\n---\nBody.\n"),
            87,
        ),
        (
            "a rule firing repeatedly still costs one rule",
            "valid-skill",
            format!(
                "---\nname: valid-skill\n{valid_body}\nauthor: a\nversion: 1\nowner: b\n---\nBody.\n"
            ),
            87,
        ),
        (
            "missing front matter leaves one rule applied, and it failed",
            "no-frontmatter",
            "# No front matter\n\nBody.\n".to_string(),
            0,
        ),
        (
            "unparseable front matter scores the same as none",
            "bad-yaml",
            "---\nname: bad-yaml\n  description: wrong indent\n---\nBody.\n".to_string(),
            0,
        ),
    ];

    for (name, dir, src, want) in cases {
        let (_base, target) = write_skill(dir, &src);
        let res = Linter::new(generous_config(), None).file(&target).unwrap();
        assert_eq!(res.score, want, "{name}");
    }
}

/// A disabled rule was never applied, so it leaves the fraction entirely;
/// --strict only moves severities, which the score does not read.
#[test]
fn score_follows_disable_but_not_strict() {
    let src = "---\nname: Bad_Name\ndescription: Use this skill for a fixture.\n---\nBody.\n";
    let (_base, target) = write_skill("Bad_Name", src);

    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(res.score, 87);

    let mut cfg = generous_config();
    cfg.strict = true;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(res.score, 87);

    let mut cfg = generous_config();
    cfg.disabled = vec![RULE_NAME_FORMAT.to_string()];
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(res.score, 100);
}

/// Full marks up to the budget, then falling linearly to zero at twice it.
#[test]
fn token_score_grades_the_overage() {
    let base = tempfile::tempdir().unwrap();
    let target = agents_target(base.path(), "Body.\n");
    let mut cfg = generous_config();
    cfg.max_agents_tokens = 100;

    for (count, want) in [
        (50, 100),
        (100, 100),
        (125, 75),
        (150, 50),
        (200, 0),
        (900, 0),
    ] {
        let linter = Linter::new(cfg.clone(), Some(Box::new(Fixed(count))));
        let res = linter.file(&target).unwrap();
        assert_eq!(res.score, want, "{count} tokens against a 100 token budget");
    }
}

/// A skill's three budgets average into one component, so name and
/// description weigh against the body rather than beside the other parts.
#[test]
fn token_score_averages_the_budgets_that_ran() {
    let (_base, target) = write_skill("valid-skill", VALID_SKILL);
    let mut cfg = generous_config();
    // Only the body is budgeted, and it sits at twice its limit.
    cfg.max_skill_tokens = 1;
    let res = Linter::new(cfg.clone(), Some(Box::new(Fixed(2))))
        .file(&target)
        .unwrap();
    // Front matter is perfect, the one budget that ran scores zero.
    assert_eq!(res.score, 50);

    // Budget the name too and it passes, pulling the token part back to 50.
    cfg.max_skill_name_tokens = 2;
    let res = Linter::new(cfg, Some(Box::new(Fixed(2))))
        .file(&target)
        .unwrap();
    assert_eq!(res.score, 75);
}

#[test]
fn file_reference_score_is_all_or_nothing() {
    let base = tempfile::tempdir().unwrap();
    write_file(base.path(), "there.md", "Present.\n");
    let target = agents_target(base.path(), "See [there](./there.md).\n");
    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(res.score, 100);

    let base = tempfile::tempdir().unwrap();
    write_file(base.path(), "there.md", "Present.\n");
    let target = agents_target(
        base.path(),
        "See [there](./there.md) and [gone](./gone.md).\n",
    );
    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(res.score, 0);
}

/// An AGENTS.md has no front matter to rate, and with every budget off and
/// nothing to resolve there is nothing left to hold against it.
#[test]
fn parts_with_nothing_to_judge_drop_out() {
    let base = tempfile::tempdir().unwrap();
    let target = agents_target(base.path(), "Body with no references.\n");
    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert_eq!(res.score, 100);

    // Give it a broken reference and a body twice its budget: the two parts
    // that now apply average, with front matter still left out.
    let target = agents_target(base.path(), "See [gone](./gone.md).\n");
    let mut cfg = generous_config();
    cfg.max_agents_tokens = 100;
    let res = Linter::new(cfg, Some(Box::new(Fixed(200))))
        .file(&target)
        .unwrap();
    assert_eq!(res.score, 0);
}
