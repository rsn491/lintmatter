//! The rule registry.
//!
//! `ALL`'s order is the one place report order is written down. It backs
//! `--list-rules`, `--disable` validation, and the order findings appear in a
//! report, so adding a rule means adding its struct here at the position it
//! should be reported -- there is no second list to keep in step.

pub mod description;
pub mod frontmatter;
pub mod name;
pub mod references;
pub mod schema;
pub mod tokens;

use super::rule::Rule;

/// Every rule, in report order.
static ALL: &[&dyn Rule] = &[
    &frontmatter::Missing,
    &frontmatter::NotFirst,
    &frontmatter::Unterminated,
    &frontmatter::Invalid,
    &frontmatter::UnknownKey,
    &name::Required,
    &name::Format,
    &name::Length,
    &name::DirMismatch,
    &description::Required,
    &description::Length,
    &schema::AllowedToolsType,
    &schema::MetadataType,
    &tokens::Content,
    &tokens::Name,
    &tokens::Description,
    &references::Missing,
];

/// Every rule, in report order.
pub fn all() -> &'static [&'static dyn Rule] {
    ALL
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for rule in all() {
            let id = rule.id();
            assert!(!id.is_empty(), "a rule has an empty id");
            assert!(seen.insert(id), "duplicate rule id {id:?}");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'),
                "rule id {id:?} is not a dotted lowercase slug"
            );
        }
    }

    #[test]
    fn every_rule_applies_to_at_least_one_kind() {
        use crate::model::Kind;
        for rule in all() {
            assert!(
                rule.applies_to(Kind::Skill) || rule.applies_to(Kind::Agents),
                "rule {:?} applies to nothing",
                rule.id()
            );
        }
    }
}
