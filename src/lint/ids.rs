//! Rule ids and the spec limits some of them enforce.

// Rule ids. Listed here so callers -- and `--disable` -- can name a rule
// without knowing which module implements it; each rule module imports the one
// it answers to. Report order is NOT set here: that lives solely in the
// registry, `rules::all`.
pub const RULE_FRONTMATTER_MISSING: &str = "frontmatter.missing";
pub const RULE_FRONTMATTER_NOT_FIRST: &str = "frontmatter.not-first";
pub const RULE_FRONTMATTER_UNTERMINATED: &str = "frontmatter.unterminated";
pub const RULE_FRONTMATTER_INVALID: &str = "frontmatter.invalid";
pub const RULE_FRONTMATTER_UNKNOWN_KEY: &str = "frontmatter.unknown-key";
pub const RULE_NAME_REQUIRED: &str = "name.required";
pub const RULE_NAME_FORMAT: &str = "name.format";
pub const RULE_NAME_LENGTH: &str = "name.length";
pub const RULE_NAME_DIR_MISMATCH: &str = "name.dir-mismatch";
pub const RULE_DESCRIPTION_REQUIRED: &str = "description.required";
pub const RULE_DESCRIPTION_LENGTH: &str = "description.length";
pub const RULE_ALLOWED_TOOLS_TYPE: &str = "allowed-tools.type";
pub const RULE_METADATA_TYPE: &str = "metadata.type";
pub const RULE_TOKENS_CONTENT: &str = "tokens.content";
pub const RULE_TOKENS_NAME: &str = "tokens.name";
pub const RULE_TOKENS_DESCRIPTION: &str = "tokens.description";
pub const RULE_FILE_REFERENCE_MISSING: &str = "file-reference.missing";

/// Limits from the Anthropic skill spec, in characters.
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
