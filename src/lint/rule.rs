//! The contract every lint rule implements.

use crate::model::Kind;

use super::{FileContext, FindingSink};

/// The part of a file's score a rule's outcome feeds. Named `Part` rather
/// than the more natural `Component` because `std::path::Component` already
/// holds that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Frontmatter,
    Tokens,
    FileReferences,
}

/// What lintmatter needs to know about a rule without running it.
///
/// Declared once per rule, beside its check, so a rule's id, the part of the
/// score it rates and the files it judges cannot drift into separate lists.
#[derive(Debug)]
pub struct RuleMeta {
    /// The id used by `--disable`, `--list-rules`, the config file and the
    /// reports.
    pub id: &'static str,
    pub part: Part,
    /// Which file kinds the rule judges.
    pub kinds: &'static [Kind],
}

/// Most rules validate the skill spec, so they judge SKILL.md alone.
pub const SKILL_ONLY: &[Kind] = &[Kind::Skill];
/// For rules whose concern holds whatever the file is.
pub const ALL_KINDS: &[Kind] = &[Kind::Agents, Kind::Skill];

impl RuleMeta {
    pub fn applies_to(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
}

/// One check lintmatter can perform.
///
/// There is one implementor per rule id, not per family, so the registry's
/// order is the only place report order is written down. A rule that has
/// nothing to say simply returns without touching the sink.
pub trait Rule: Sync {
    const META: RuleMeta;

    /// Runs the check. The sink already knows which rule is running, so
    /// implementors only describe what is wrong and where.
    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>);
}

/// [`Rule`] as the registry holds it. An associated const keeps a trait from
/// being used as `dyn`, so rules implement `Rule` and this blanket impl lets
/// the registry hold them side by side.
pub trait DynRule: Sync {
    fn meta(&self) -> &'static RuleMeta;
    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>);
}

impl<R: Rule> DynRule for R {
    fn meta(&self) -> &'static RuleMeta {
        &R::META
    }

    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>) {
        Rule::check(self, ctx, sink);
    }
}
