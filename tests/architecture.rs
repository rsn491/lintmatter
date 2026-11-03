//! Enforces the ports-and-adapters dependency rule: `domain` depends on
//! nothing else in the crate, `application` only on `domain`, and neither
//! reaches the filesystem, the environment or the command line directly.
//! Adapters are free to depend on everything.
//!
//! Test code is exempt: unit-test modules and `test_utils.rs` fixtures may
//! touch the disk to build fixtures.

use std::fs;
use std::path::{Path, PathBuf};

/// I/O and framework entry points no inner layer may name.
const INFRASTRUCTURE: &[&str] = &["std::fs", "std::env", "std::process", "clap", "glob::"];

fn sources(layer: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = vec![root.join(format!("{layer}.rs"))];
    let mut stack = vec![root.join(layer)];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "test_utils.rs")
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The non-test, non-comment lines of a source file, numbered from 1.
fn production_lines(path: &Path) -> Vec<(usize, String)> {
    let src = fs::read_to_string(path).unwrap();
    let src = match src.find("#[cfg(test)]\nmod tests") {
        Some(i) => &src[..i],
        None => &src[..],
    };
    src.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect()
}

fn violations(layer: &str, forbidden: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for path in sources(layer) {
        for (n, line) in production_lines(&path) {
            for pat in forbidden {
                if line.contains(pat) {
                    found.push(format!(
                        "{}:{n}: names {pat:?}: {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    found
}

#[test]
fn domain_depends_on_nothing_outside_itself() {
    let mut forbidden = vec!["crate::application", "crate::adapters"];
    forbidden.extend(INFRASTRUCTURE);
    let found = violations("domain", &forbidden);
    assert!(
        found.is_empty(),
        "domain breaks the dependency rule:\n{}",
        found.join("\n")
    );
}

#[test]
fn application_depends_only_on_the_domain() {
    let mut forbidden = vec!["crate::adapters"];
    forbidden.extend(INFRASTRUCTURE);
    let found = violations("application", &forbidden);
    assert!(
        found.is_empty(),
        "application breaks the dependency rule:\n{}",
        found.join("\n")
    );
}

/// Guards the guard: a walk that silently found no files would pass forever.
#[test]
fn every_layer_has_sources() {
    for layer in ["domain", "application", "adapters"] {
        assert!(sources(layer).len() > 1, "no sources found for {layer}");
    }
}
