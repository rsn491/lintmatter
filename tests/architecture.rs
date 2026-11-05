//! Enforces the ports-and-adapters dependency rule by parsing every source file
//! and checking each path it names:
//!
//! - `domain` may name only itself and `utils`, never touches the filesystem,
//!   the environment or the process, and may use only the external crates it
//!   needs to parse and measure a file.
//! - `utils` is a leaf: it names nothing else in the crate.
//! - `application` may also name `domain`, and nothing in `adapters`.
//! - `adapters` may depend on everything, so they are not checked.
//!
//! External crates are allowlisted per layer, read against `Cargo.toml`'s
//! `[dependencies]`, so a newly added dependency is kept out of the inner
//! layers until someone decides otherwise. Test code is exempt: items under
//! `#[cfg(test)]` and `test_utils.rs` fixtures may touch the disk.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Item, ItemMod, Macro, Meta, Token, UseTree};

/// What one layer may name.
struct Layer {
    name: &'static str,
    /// Top-level modules of this crate the layer may reach.
    internal: &'static [&'static str],
    /// External crates from `[dependencies]` the layer may use.
    external: &'static [&'static str],
    /// `std` modules that reach the outside world.
    banned_std: &'static [&'static str],
}

/// `std::io` is not banned: the ports speak `io::Result` and `io::Error`.
const IO: &[&str] = &["fs", "env", "process", "net", "thread"];

const DOMAIN: Layer = Layer {
    name: "domain",
    internal: &["domain", "utils"],
    external: &["regex", "saphyr"],
    banned_std: IO,
};

const UTILS: Layer = Layer {
    name: "utils",
    internal: &["utils"],
    external: &[],
    banned_std: IO,
};

const APPLICATION: Layer = Layer {
    name: "application",
    internal: &["application", "domain", "utils"],
    external: &["thiserror"],
    banned_std: IO,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The `[dependencies]` of this crate, as they are spelled in paths.
fn dependencies() -> HashSet<String> {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let mut in_section = false;
    let mut deps = HashSet::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[dependencies]";
            continue;
        }
        if in_section && let Some((name, _)) = line.split_once('=') {
            deps.insert(name.trim().replace('-', "_"));
        }
    }
    assert!(!deps.is_empty(), "no [dependencies] found in Cargo.toml");
    deps
}

/// The non-fixture source files of a layer: `src/<layer>.rs` and everything
/// under `src/<layer>/`.
fn sources(layer: &str) -> Vec<PathBuf> {
    let src = root().join("src");
    let mut out = vec![src.join(format!("{layer}.rs"))];
    let mut stack = vec![src.join(layer)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
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

/// `src/domain/lint/rules/name.rs` is the module `domain::lint::rules::name`.
fn module_path(file: &Path) -> Vec<String> {
    let rel = file.strip_prefix(root().join("src")).unwrap();
    rel.with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}

/// A path the source names, where it names it.
struct Named {
    segments: Vec<String>,
    line: usize,
    /// Came from a `use` item or a derive, where even a single segment names a
    /// crate. A lone identifier in an expression is more likely a variable.
    import: bool,
}

struct Collector {
    module: Vec<String>,
    found: Vec<Named>,
}

fn line_of(span: Span) -> usize {
    span.start().line
}

fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && matches!(&a.meta, Meta::List(l) if l.tokens.to_string() == "test")
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

impl Collector {
    fn push(&mut self, segments: Vec<String>, line: usize, import: bool) {
        self.found.push(Named {
            segments,
            line,
            import,
        });
    }

    fn use_tree(&mut self, prefix: &mut Vec<String>, tree: &UseTree) {
        match tree {
            UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                self.use_tree(prefix, &p.tree);
                prefix.pop();
            }
            UseTree::Name(n) => {
                let mut segs = prefix.clone();
                // `use std::io::{self, Write}` imports `std::io` itself.
                if n.ident != "self" {
                    segs.push(n.ident.to_string());
                }
                self.push(segs, line_of(n.ident.span()), true);
            }
            UseTree::Rename(r) => {
                let mut segs = prefix.clone();
                if r.ident != "self" {
                    segs.push(r.ident.to_string());
                }
                self.push(segs, line_of(r.ident.span()), true);
            }
            UseTree::Glob(g) => {
                self.push(prefix.clone(), line_of(g.star_token.span), true);
            }
            UseTree::Group(g) => {
                for t in &g.items {
                    self.use_tree(prefix, t);
                }
            }
        }
    }

    /// Macro bodies are opaque to the parser, so `format!("{}", std::fs::x())`
    /// would otherwise go unseen: pick out every `a::b::c` run in the tokens.
    fn tokens(&mut self, stream: TokenStream) {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        let mut i = 0;
        while i < tokens.len() {
            if let TokenTree::Group(g) = &tokens[i] {
                self.tokens(g.stream());
                i += 1;
                continue;
            }
            let TokenTree::Ident(first) = &tokens[i] else {
                i += 1;
                continue;
            };
            let mut segs = vec![first.to_string()];
            let mut j = i + 1;
            while let (
                Some(TokenTree::Punct(a)),
                Some(TokenTree::Punct(b)),
                Some(TokenTree::Ident(id)),
            ) = (tokens.get(j), tokens.get(j + 1), tokens.get(j + 2))
            {
                if a.as_char() != ':' || b.as_char() != ':' {
                    break;
                }
                segs.push(id.to_string());
                j += 3;
            }
            if segs.len() > 1 {
                self.push(segs, line_of(first.span()), false);
            }
            i = j;
        }
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, item: &'ast Item) {
        if !is_cfg_test(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        if m.content.is_some() {
            self.module.push(m.ident.to_string());
            visit::visit_item_mod(self, m);
            self.module.pop();
        } else {
            visit::visit_item_mod(self, m);
        }
    }

    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut prefix = Vec::new();
        self.use_tree(&mut prefix, &u.tree);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        if let Some(first) = p.segments.first() {
            self.push(segs, line_of(first.ident.span()), false);
        }
        visit::visit_path(self, p);
    }

    fn visit_attribute(&mut self, a: &'ast Attribute) {
        if a.path().is_ident("derive") {
            let paths = a
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .unwrap_or_default();
            for p in paths {
                let segs = p.segments.iter().map(|s| s.ident.to_string()).collect();
                let line = p.segments.first().map_or(0, |s| line_of(s.ident.span()));
                self.push(segs, line, true);
            }
        } else if let Some(id) = a.path().get_ident() {
            // A helper attribute such as `#[serde(...)]` names its crate.
            self.push(vec![id.to_string()], line_of(id.span()), true);
        }
        visit::visit_attribute(self, a);
    }

    fn visit_macro(&mut self, m: &'ast Macro) {
        self.tokens(m.tokens.clone());
        visit::visit_macro(self, m);
    }
}

/// Resolves `crate::`, `self::` and `super::` against `module`, returning the
/// path from the crate root, or `None` when the path does not start in this
/// crate.
fn resolve_internal(module: &[String], segs: &[String]) -> Option<Vec<String>> {
    match segs.first()?.as_str() {
        "crate" => Some(segs[1..].to_vec()),
        "self" => Some(module.iter().chain(&segs[1..]).cloned().collect()),
        "super" => {
            let ups = segs.iter().take_while(|s| *s == "super").count();
            let base = module.len().checked_sub(ups)?;
            Some(module[..base].iter().chain(&segs[ups..]).cloned().collect())
        }
        _ => None,
    }
}

/// Every path in `src` that `layer` may not name, as `line: reason` strings.
fn check_source(
    src: &str,
    module: &[String],
    layer: &Layer,
    deps: &HashSet<String>,
) -> Vec<String> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("cannot parse: {e}"));
    let mut c = Collector {
        module: module.to_vec(),
        found: Vec::new(),
    };
    c.visit_file(&file);

    let mut out = Vec::new();
    for Named {
        segments,
        line,
        import,
    } in c.found
    {
        let shown = segments.join("::");
        let Some(first) = segments.first() else {
            continue;
        };
        if let Some(abs) = resolve_internal(module, &segments) {
            if let Some(top) = abs.first()
                && !layer.internal.contains(&top.as_str())
            {
                out.push(format!("{line}: names {shown} (crate::{top})"));
            }
        } else if matches!(first.as_str(), "std" | "core" | "alloc") {
            if let Some(m) = segments.get(1)
                && layer.banned_std.contains(&m.as_str())
            {
                out.push(format!(
                    "{line}: names {shown} ({first}::{m} reaches the outside world)"
                ));
            }
        } else if (import || segments.len() > 1)
            && deps.contains(first)
            && !layer.external.contains(&first.as_str())
        {
            out.push(format!(
                "{line}: names {shown} (crate {first} is not allowed here)"
            ));
        }
    }
    out.dedup();
    out
}

fn check_files(files: &[PathBuf], layer: &Layer) -> Vec<String> {
    let deps = dependencies();
    let mut found = Vec::new();
    for path in files {
        let src = fs::read_to_string(path).unwrap();
        for v in check_source(&src, &module_path(path), layer, &deps) {
            found.push(format!("{}:{v}", path.display()));
        }
    }
    found
}

fn assert_clean(files: &[PathBuf], layer: &Layer) {
    let found = check_files(files, layer);
    assert!(
        found.is_empty(),
        "{} breaks the dependency rule:\n{}",
        layer.name,
        found.join("\n")
    );
}

#[test]
fn domain_depends_on_nothing_outside_itself() {
    assert_clean(&sources("domain"), &DOMAIN);
}

#[test]
fn utils_is_a_leaf() {
    assert_clean(&[root().join("src/utils.rs")], &UTILS);
}

#[test]
fn application_depends_only_on_the_domain() {
    assert_clean(&sources("application"), &APPLICATION);
}

/// Guards the guard: a walk that silently found no files would pass forever.
#[test]
fn every_layer_has_sources() {
    for layer in ["domain", "application", "adapters"] {
        assert!(sources(layer).len() > 1, "no sources found for {layer}");
    }
    assert!(root().join("src/utils.rs").is_file());
}

/// Guards the guard: each of these slipped past the substring check this
/// replaced, or would slip past a parser that only read `use` items.
#[test]
fn the_checker_catches_each_way_of_naming_a_forbidden_path() {
    let deps = dependencies();
    let module = |m: &str| m.split("::").map(String::from).collect::<Vec<_>>();
    let cases: &[(&str, &str, &Layer, &str)] = &[
        (
            "grouped std import",
            "use std::{env, path::Path};",
            &DOMAIN,
            "std::env",
        ),
        (
            "std in an expression",
            "fn f() { let _ = std::fs::read(\"x\"); }",
            &DOMAIN,
            "std::fs",
        ),
        (
            "std inside a macro",
            "fn f() { println!(\"{:?}\", std::env::args()); }",
            &DOMAIN,
            "std::env",
        ),
        (
            "relative climb",
            "use super::super::adapters::cli;",
            &APPLICATION,
            "crate::adapters",
        ),
        (
            "absolute adapter",
            "fn f(_: crate::adapters::fs::LocalFs) {}",
            &APPLICATION,
            "crate::adapters",
        ),
        (
            "application from domain",
            "use crate::application::Settings;",
            &DOMAIN,
            "crate::application",
        ),
        (
            "derive from a disallowed crate",
            "#[derive(serde::Serialize)] struct S;",
            &DOMAIN,
            "crate serde",
        ),
        (
            "helper attribute",
            "#[serde(rename_all = \"lowercase\")] enum E { A }",
            &DOMAIN,
            "crate serde",
        ),
        (
            "disallowed crate",
            "use clap::Parser;",
            &APPLICATION,
            "crate clap",
        ),
        (
            "utils reaching in",
            "use crate::domain::Kind;",
            &UTILS,
            "crate::domain",
        ),
    ];
    for (name, src, layer, want) in cases {
        let found = check_source(src, &module("application::lint_run"), layer, &deps);
        assert!(
            found.iter().any(|f| f.contains(want)),
            "{name}: expected a finding naming {want}, got {found:?}"
        );
    }
}

/// And the converse: none of these may be reported, or the guard would be
/// silenced by an allow-everything patch the first time it misfired.
#[test]
fn the_checker_accepts_what_a_layer_may_name() {
    let deps = dependencies();
    let module = vec!["domain".to_string(), "lint".to_string()];
    let cases: &[(&str, &str)] = &[
        (
            "own layer",
            "use crate::domain::Kind; use super::Kind as K; use self::rule::Rule;",
        ),
        (
            "std io",
            "use std::io::{self, Write}; fn f() -> io::Result<()> { Ok(()) }",
        ),
        ("allowed crate", "use regex::Regex;"),
        (
            "test code",
            "#[cfg(test)] mod tests { use std::fs; use crate::adapters::cli; }",
        ),
        (
            "local variable sharing a crate name",
            "fn f(glob: u8) -> u8 { glob }",
        ),
    ];
    for (name, src) in cases {
        let found = check_source(src, &module, &DOMAIN, &deps);
        assert!(found.is_empty(), "{name}: unexpected findings {found:?}");
    }
}
