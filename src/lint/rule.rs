//! The contract every lint rule implements.

use crate::discover::Kind;

use super::{FileContext, FindingSink};

/// One check lintmatter can perform.
///
/// There is one implementor per rule id, not per family, so the registry's
/// order is the only place report order is written down: it drives
/// `--list-rules`, `--disable` validation, and the order findings come out in.
/// A rule that has nothing to say simply returns without touching the sink.
pub trait Rule: Sync {
    /// The id used by `--disable`, `--list-rules` and the reports.
    fn id(&self) -> &'static str;

    /// Which file kinds this rule judges. Most rules validate the skill spec
    /// and so apply to SKILL.md alone, which is the default.
    fn applies_to(&self, kind: Kind) -> bool {
        kind == Kind::Skill
    }

    /// Runs the check. The sink already knows this rule's id and handles
    /// `--strict`, so implementors only describe what is wrong and where.
    fn check(&self, ctx: &FileContext<'_>, sink: &mut FindingSink<'_>);
}
