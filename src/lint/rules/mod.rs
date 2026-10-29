//! The rule registry.
//!
//! `ALL`'s order is the one place report order is written down. It backs
//! `--list-rules`, `--disable` validation, the config file's `rules:` mapping,
//! and the order findings appear in a report, so adding a rule means adding
//! its struct here at the position it should be reported -- there is no
//! second list to keep in step.

pub mod description;
pub mod frontmatter;
pub mod name;
pub mod references;
pub mod schema;
pub mod tokens;

use super::rule::DynRule;

/// Renders n with thousands separators, so "6,142 tokens" reads at a glance
/// in a finding's message.
pub(crate) fn humanize(n: i64) -> String {
    let s = n.to_string();
    if n < 0 {
        return s;
    }
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Every rule, in report order.
static ALL: &[&dyn DynRule] = &[
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
pub fn all() -> &'static [&'static dyn DynRule] {
    ALL
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn humanize_formats_with_separators() {
        let cases: &[(i64, &str)] = &[
            (0, "0"),
            (7, "7"),
            (999, "999"),
            (1000, "1,000"),
            (6142, "6,142"),
            (1234567, "1,234,567"),
            (-42, "-42"),
        ];
        for (n, want) in cases {
            assert_eq!(humanize(*n), *want, "humanize({n})");
        }
    }

    #[test]
    fn ids_are_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for rule in all() {
            let id = rule.meta().id;
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
        for rule in all() {
            assert!(
                !rule.meta().kinds.is_empty(),
                "rule {:?} applies to nothing",
                rule.meta().id
            );
        }
    }
}
