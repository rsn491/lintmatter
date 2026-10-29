use super::*;
use crate::config::{
    DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS, DEFAULT_MAX_SKILL_NAME_TOKENS,
    DEFAULT_MAX_SKILL_TOKENS,
};
use crate::lint::*;

#[test]
fn token_budgets() {
    let (_base, target) = write_skill("budgeted", VALID_SKILL);

    let mut cfg = generous_config();
    cfg.max_skill_tokens = 1;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(
        rule_ids(&res.findings),
        vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_CONTENT]
    );

    let mut cfg = generous_config();
    cfg.max_skill_tokens = 0;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(rule_ids(&res.findings), vec![RULE_NAME_DIR_MISMATCH]);

    let mut cfg = generous_config();
    cfg.max_skill_name_tokens = 1;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(
        rule_ids(&res.findings),
        vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_NAME]
    );

    let mut cfg = generous_config();
    cfg.max_skill_description_tokens = 2;
    let res = Linter::new(cfg, None).file(&target).unwrap();
    assert_eq!(
        rule_ids(&res.findings),
        vec![RULE_NAME_DIR_MISMATCH, RULE_TOKENS_DESCRIPTION]
    );

    let res = Linter::new(generous_config(), None).file(&target).unwrap();
    assert!(res.tokens.content > 0 && res.tokens.name > 0 && res.tokens.description > 0);
}

#[test]
fn per_kind_content_budget() {
    let base = tempfile::tempdir().unwrap();
    let body = "some filler prose to spend tokens on. ".repeat(20);
    let agents = agents_target(base.path(), &body);
    let (_skill_base, skill) = write_skill(
        "kinded",
        &format!("---\nname: kinded\ndescription: A skill with a body.\n---\n{body}"),
    );

    let mut cfg = generous_config();
    cfg.max_skill_tokens = 10000;
    cfg.max_agents_tokens = 1;
    let linter = Linter::new(cfg.clone(), None);

    let agents_res = linter.file(&agents).unwrap();
    assert_eq!(agents_res.errors(), 1);

    let skill_res = linter.file(&skill).unwrap();
    assert!(
        !skill_res
            .findings
            .iter()
            .any(|f| f.rule == RULE_TOKENS_CONTENT)
    );

    cfg.max_agents_tokens = 0;
    cfg.max_skill_tokens = 1;
    let skill_res = Linter::new(cfg, None).file(&skill).unwrap();
    assert!(
        skill_res
            .findings
            .iter()
            .any(|f| f.rule == RULE_TOKENS_CONTENT)
    );
}

#[test]
fn default_config_enforces_budgets() {
    let cfg = Config::default();
    assert_eq!(cfg.max_agents_tokens, DEFAULT_MAX_AGENTS_TOKENS);
    assert_eq!(cfg.max_skill_tokens, DEFAULT_MAX_SKILL_TOKENS);
    assert_eq!(cfg.max_skill_name_tokens, DEFAULT_MAX_SKILL_NAME_TOKENS);
    assert_eq!(
        cfg.max_skill_description_tokens,
        DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS
    );

    // The point of the hand-written Default: reaching for it must not
    // hand back a linter that checks nothing.
    let base = tempfile::tempdir().unwrap();
    let body = "some filler prose to spend tokens on. ".repeat(500);
    let target = agents_target(base.path(), &body);
    let res = Linter::new(Config::default(), None).file(&target).unwrap();
    assert_eq!(rule_ids(&res.findings), vec![RULE_TOKENS_CONTENT]);
}
