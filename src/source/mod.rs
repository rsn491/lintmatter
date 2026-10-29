//! Turns paths on disk into documents to lint: which files to read
//! ([`discover`]), how one splits into front matter and body ([`parse`]), and
//! the line-level helpers both need.

pub mod discover;
pub mod fence;
pub mod parse;
pub mod paths;
