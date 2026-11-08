//! Splits a markdown file into YAML front matter and body, keeping enough
//! position information to anchor lint findings to the right line.

use std::collections::HashMap;

use regex::Regex;
use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
use std::sync::LazyLock;

/// Classifies a malformed front-matter block. The linter maps each kind onto a
/// rule id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrKind {
    /// The fence exists but something precedes it.
    NotFirst,
    /// The opening fence is never closed.
    Unterminated,
    /// The block is not parseable YAML.
    InvalidYaml,
    /// The block parses but is not a key/value mapping.
    NotMapping,
}

/// Describes why a front-matter block could not be used.
#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrKind,
    /// 1-based line in the file, 0 when unknown.
    pub line: usize,
    pub msg: String,
}

/// An owned, simplified YAML value: enough shape information for lintmatter's
/// rules without borrowing from the source text.
#[derive(Debug, Clone)]
pub enum Value {
    Scalar(String),
    Sequence(Vec<Value>),
    Mapping,
    Other,
}

struct Entry {
    key_line: usize,
    value: Value,
}

/// A parsed front-matter block.
#[derive(Default)]
pub struct Frontmatter {
    /// Whether the file opened with a front-matter fence.
    pub present: bool,
    /// 1-based line of the closing fence, 0 when absent.
    pub end_line: usize,

    keys: Vec<String>,
    entries: HashMap<String, Entry>,
}

impl Frontmatter {
    /// Returns the mapping keys in document order.
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Reports whether key is present, regardless of its value.
    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Returns the 1-based line where key is declared, or end_line as a
    /// fallback when the key is absent.
    pub fn line(&self, key: &str) -> usize {
        self.entries.get(key).map_or(self.end_line, |e| e.key_line)
    }

    /// Returns the value node for key, or None when the key is absent.
    pub fn node(&self, key: &str) -> Option<&Value> {
        self.entries.get(key).map(|e| &e.value)
    }

    /// Returns the scalar value of key. None when the key is absent or its
    /// value is not a scalar.
    pub fn string(&self, key: &str) -> Option<String> {
        match self.node(key) {
            Some(Value::Scalar(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Returns the value of key as a list of strings, accepting either a YAML
    /// sequence of scalars or a single comma-separated scalar. None when the
    /// key is absent or holds neither shape.
    pub fn string_slice(&self, key: &str) -> Option<Vec<String>> {
        match self.node(key)? {
            Value::Scalar(s) => Some(
                s.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            Value::Sequence(items) => items
                .iter()
                .map(|item| match item {
                    Value::Scalar(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            Value::Mapping | Value::Other => None,
        }
    }
}

const BOM: char = '\u{feff}';

static OPEN_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-{3,}[ \t]*$").expect("valid regex literal"));
static CLOSE_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(-{3,}|\.{3,})[ \t]*$").expect("valid regex literal"));

/// Joins a range of lines; a range past the end yields nothing rather than panicking.
fn join_lines(lines: &[&str], range: std::ops::Range<usize>) -> String {
    lines.get(range).map_or_else(String::new, |l| l.join("\n"))
}

fn fence_line(line: &str) -> &str {
    line.trim_end_matches('\r')
}

/// A markdown file split into its front matter and body.
///
/// A three-value tuple made every caller destructure positionally and gave the
/// pieces no name; this gives them one. `error` describes a malformed block:
/// `body` is still populated in that case, so token budgets can be reported
/// for a file whose front matter did not parse.
pub struct Document {
    pub frontmatter: Frontmatter,
    pub body: String,
    pub error: Option<Error>,
}

impl Document {
    /// Parses one markdown file. A file with no front matter is not an error:
    /// `frontmatter.present` is false and `body` is the whole file.
    pub fn parse(src: &[u8]) -> Document {
        let (frontmatter, body, error) = split(src);
        Document {
            frontmatter,
            body,
            error,
        }
    }
    /// The front matter, but only when it is actually usable: present, parsed,
    /// and a mapping. Rules that read keys go through this so a malformed
    /// block short-circuits them uniformly.
    pub fn mapping(&self) -> Option<&Frontmatter> {
        if self.error.is_some() || !self.frontmatter.present {
            return None;
        }
        Some(&self.frontmatter)
    }
}

/// Separates the front matter of a markdown file from its body.
///
/// A file with no front matter is not an error: `Frontmatter::present` is
/// false and body is the whole file. A malformed block yields an `Error` and a
/// best-effort body so token budgets can still be reported.
fn split(src: &[u8]) -> (Frontmatter, String, Option<Error>) {
    let text = String::from_utf8_lossy(src);
    let text = text.strip_prefix(BOM).unwrap_or(&text).to_string();
    let lines: Vec<&str> = text.split('\n').collect();

    let open: isize = lines
        .iter()
        .enumerate()
        .find(|(_, line)| !line.trim().is_empty())
        .filter(|(_, line)| OPEN_FENCE_RE.is_match(fence_line(line)))
        .map_or(-1, |(i, _)| i as isize);
    if open == -1 {
        // A `---` block further down may be misplaced front matter: say so.
        if let Some(line) = misplaced_frontmatter(&lines) {
            return (
                Frontmatter::default(),
                text.clone(),
                Some(Error {
                    kind: ErrKind::NotFirst,
                    line,
                    msg: "front matter must start on the first line of the file".to_string(),
                }),
            );
        }
        return (Frontmatter::default(), text, None);
    }
    if open > 0 {
        return (
            Frontmatter::default(),
            text.clone(),
            Some(Error {
                kind: ErrKind::NotFirst,
                line: (open + 1) as usize,
                msg: "front matter must start on the first line of the file".to_string(),
            }),
        );
    }
    let open = open as usize;

    let close_idx = lines
        .iter()
        .enumerate()
        .skip(open + 1)
        .find(|(_, line)| CLOSE_FENCE_RE.is_match(fence_line(line)))
        .map(|(i, _)| i);
    let Some(close_idx) = close_idx else {
        // The fence never counts toward the content budget, even unterminated.
        let body = join_lines(&lines, open + 1..lines.len());
        return (
            Frontmatter {
                present: true,
                ..Default::default()
            },
            body,
            Some(Error {
                kind: ErrKind::Unterminated,
                line: open + 1,
                msg: "front matter is not closed by a --- fence".to_string(),
            }),
        );
    };

    let block = join_lines(&lines, open + 1..close_idx);
    let body = join_lines(&lines, close_idx + 1..lines.len());

    let mut fm = Frontmatter {
        present: true,
        end_line: close_idx + 1,
        ..Default::default()
    };

    if let Some(why) = unsafe_yaml(&block) {
        return (
            fm,
            body,
            Some(Error {
                kind: ErrKind::InvalidYaml,
                line: open + 1,
                msg: format!("invalid YAML in front matter: {why}"),
            }),
        );
    }
    let docs = match MarkedYaml::load_from_str(&block) {
        Ok(docs) => docs,
        Err(e) => {
            return (
                fm,
                body,
                Some(Error {
                    kind: ErrKind::InvalidYaml,
                    line: e.marker().line() + open + 1,
                    msg: format!("invalid YAML in front matter: {}", e.info().trim()),
                }),
            );
        }
    };

    let Some(root) = docs.into_iter().next() else {
        return (
            fm,
            body,
            Some(Error {
                kind: ErrKind::NotMapping,
                line: open + 1,
                msg: "front matter is empty".to_string(),
            }),
        );
    };

    let YamlData::Mapping(map) = &root.data else {
        return (
            fm,
            body,
            Some(Error {
                kind: ErrKind::NotMapping,
                line: root.span.start.line() + open + 1,
                msg: "front matter must be a mapping of keys to values".to_string(),
            }),
        );
    };

    map.iter()
        .filter_map(|(key, value)| Some((scalar_string(&key.data)?, key, value)))
        .for_each(|(key_str, key, value)| {
            let key_line = key.span.start.line() + open + 1;
            if !fm.entries.contains_key(&key_str) {
                fm.keys.push(key_str.clone());
            }
            fm.entries.insert(
                key_str,
                Entry {
                    key_line,
                    value: to_value(value),
                },
            );
        });

    (fm, body, None)
}

/// Finds a `---` block below the top that is really misplaced front matter,
/// returning its 1-based opening line. Strict (must close, parse as a mapping,
/// and carry `name` or `description`) so a markdown thematic break never matches.
fn misplaced_frontmatter(lines: &[&str]) -> Option<usize> {
    lines.iter().enumerate().find_map(|(i, line)| {
        if !OPEN_FENCE_RE.is_match(fence_line(line)) {
            return None;
        }
        let close = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .find(|(_, l)| CLOSE_FENCE_RE.is_match(fence_line(l)))
            .map(|(j, _)| j)?;
        let block = join_lines(lines, i + 1..close);
        if unsafe_yaml(&block).is_some() {
            return None;
        }
        let docs = MarkedYaml::load_from_str(&block).ok()?;
        let root = docs.into_iter().next()?;
        let YamlData::Mapping(map) = &root.data else {
            return None;
        };
        let names_a_skill = map.keys().any(|k| {
            scalar_string(&k.data).is_some_and(|key| key == "name" || key == "description")
        });
        names_a_skill.then_some(i + 1)
    })
}

/// The most YAML we parse from one block; real front matter and config are a
/// few hundred bytes.
const MAX_YAML_BYTES: usize = 64 * 1024;
const MAX_YAML_DEPTH: usize = 32;

/// Why `src` must not reach the YAML parser, if it must not. The parser
/// deep-copies an anchored node at every alias and has no depth limit, so a few
/// hundred bytes of nested aliases or brackets can exhaust memory or the stack
/// of a run over a tree it does not trust. Aliases have no use in instruction
/// files, so they are refused rather than bounded. Shared with the config loader.
pub fn unsafe_yaml(src: &str) -> Option<&'static str> {
    if src.len() > MAX_YAML_BYTES {
        return Some("YAML block is larger than 64 KiB");
    }
    let mut flow = 0usize;
    for line in src.lines() {
        let indent = line.len() - line.trim_start().len();
        if indent > MAX_YAML_DEPTH * 4 {
            return Some("YAML is nested too deeply");
        }
        let mut rest = line.trim_start();
        while let Some(r) = rest.strip_prefix("- ") {
            rest = r.trim_start();
        }
        if let Some((_, value)) = rest.split_once(": ") {
            rest = value.trim_start();
        }
        // `* item` is a bullet in a block scalar; an alias names an anchor.
        let alias = |s: &str| {
            s.strip_prefix('*')
                .is_some_and(|n| n.starts_with(|c: char| !c.is_whitespace()))
        };
        if alias(rest) {
            return Some("YAML aliases are not supported");
        }
        let mut node_start = true;
        for (i, c) in rest.char_indices() {
            match c {
                '[' | '{' => {
                    flow += 1;
                    if flow > MAX_YAML_DEPTH {
                        return Some("YAML is nested too deeply");
                    }
                    node_start = true;
                }
                ']' | '}' => {
                    flow = flow.saturating_sub(1);
                    node_start = false;
                }
                ',' => node_start = true,
                '*' if node_start && alias(&rest[i..]) => {
                    return Some("YAML aliases are not supported");
                }
                c if c.is_whitespace() => {}
                _ => node_start = false,
            }
        }
    }
    None
}

/// Renders a scalar YAML node as a string. Shared with the config loader,
/// which reads the same flavor of YAML.
pub fn scalar_string<'a>(data: &YamlData<'a, MarkedYaml<'a>>) -> Option<String> {
    match data {
        YamlData::Value(scalar) => Some(stringify_scalar(scalar)),
        YamlData::Representation(cow, ..) => Some(cow.to_string()),
        _ => None,
    }
}

fn stringify_scalar(scalar: &Scalar) -> String {
    match scalar {
        Scalar::Null => "null".to_string(),
        Scalar::Boolean(b) => b.to_string(),
        Scalar::Integer(i) => i.to_string(),
        Scalar::FloatingPoint(f) => f.into_inner().to_string(),
        Scalar::String(s) => s.to_string(),
    }
}

fn to_value(node: &MarkedYaml) -> Value {
    match &node.data {
        YamlData::Value(scalar) => Value::Scalar(stringify_scalar(scalar)),
        YamlData::Representation(cow, ..) => Value::Scalar(cow.to_string()),
        YamlData::Sequence(items) => Value::Sequence(items.iter().map(to_value).collect()),
        YamlData::Mapping(_) => Value::Mapping,
        YamlData::Alias(_) | YamlData::BadValue | YamlData::Tagged(..) => Value::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_valid() {
        // Given
        let src =
            "---\nname: my-skill\ndescription: Does a thing.\n---\n\n# Heading\n\nBody text.\n";

        // When
        let d = Document::parse(src.as_bytes());

        // Then
        let (fm, body, err) = (d.frontmatter, d.body, d.error);
        assert!(err.is_none());
        assert!(fm.present);
        assert_eq!(fm.keys(), &["name".to_string(), "description".to_string()]);
        assert_eq!(fm.string("name").as_deref(), Some("my-skill"));
        assert_eq!(fm.line("name"), 2);
        assert_eq!(fm.line("description"), 3);
        assert_eq!(fm.end_line, 4);
        assert!(body.starts_with("\n# Heading"));
        assert!(!body.contains("name:"));
    }

    #[test]
    fn split_no_frontmatter() {
        // Given
        let src = "# Just markdown\n\nNo front matter here.\n";

        // When
        let d = Document::parse(src.as_bytes());

        // Then
        let (fm, body, err) = (d.frontmatter, d.body, d.error);
        assert!(err.is_none());
        assert!(!fm.present);
        assert_eq!(body, src);
    }

    #[test]
    fn split_bom() {
        // Given
        let src = "\u{feff}---\nname: bom-skill\n---\nbody\n";

        // When
        let d = Document::parse(src.as_bytes());

        // Then
        let (fm, _, err) = (d.frontmatter, d.body, d.error);
        assert!(err.is_none());
        assert_eq!(fm.string("name").as_deref(), Some("bom-skill"));
    }

    #[test]
    fn split_crlf() {
        // Given
        let src = "---\r\nname: crlf-skill\r\n---\r\nbody\r\n";

        // When
        let d = Document::parse(src.as_bytes());

        // Then
        let (fm, _, err) = (d.frontmatter, d.body, d.error);
        assert!(err.is_none());
        assert_eq!(fm.string("name").as_deref(), Some("crlf-skill"));
    }

    #[test]
    fn split_errors() {
        // Given
        let cases: &[(&str, &str, ErrKind, usize)] = &[
            (
                "fence not first",
                "\n---\nname: late\n---\n",
                ErrKind::NotFirst,
                2,
            ),
            (
                "unterminated",
                "---\nname: open-ended\nbody without a closing fence\n",
                ErrKind::Unterminated,
                1,
            ),
            (
                "invalid yaml",
                "---\nname: ok\n  bad: indent\n---\nbody\n",
                ErrKind::InvalidYaml,
                3,
            ),
            (
                "not a mapping",
                "---\n- one\n- two\n---\nbody\n",
                ErrKind::NotMapping,
                2,
            ),
            ("empty block", "---\n---\nbody\n", ErrKind::NotMapping, 1),
        ];
        for (name, src, want_kind, want_line) in cases {
            // When
            let d = Document::parse(src.as_bytes());

            // Then
            let (_, body, err) = (d.frontmatter, d.body, d.error);
            let err = err.unwrap_or_else(|| panic!("{name}: want an error"));
            assert_eq!(err.kind, *want_kind, "{name}: kind");
            assert_eq!(err.line, *want_line, "{name}: line (message: {})", err.msg);
            assert!(!body.is_empty(), "{name}: body should be non-empty");
            assert!(
                !err.msg.contains('\n'),
                "{name}: message should be one line"
            );
        }
    }

    #[test]
    fn misplaced_frontmatter_is_distinguished_from_a_thematic_break() {
        // Given
        let cases: &[(&str, &str)] = &[
            (
                "after a heading",
                "# My Skill\n\n---\nname: my-skill\ndescription: Does a thing.\n---\nBody.\n",
            ),
            (
                "after prose",
                "Some intro.\n\n---\ndescription: Does a thing.\n---\nBody.\n",
            ),
        ];
        let unclosed_candidate: &[u8] = b"# Title\n\n---\n\nA break with no partner.\n\n---\nname: late-skill\ndescription: Real front matter, in the wrong place.\n---\n";
        let quiet: &[(&str, &str)] = &[
            ("thematic break", "# Title\n\n---\n\nA new section.\n"),
            (
                "setext underline",
                "Title\n---\n\nBody text under a setext heading.\n",
            ),
            (
                "block without skill keys",
                "# Title\n\n---\nfoo: bar\n---\n\nBody.\n",
            ),
            (
                "block that never closes",
                "# Title\n\n---\nname: unclosed\n",
            ),
            (
                "unparseable block",
                "# Title\n\n---\nname: ok\n  bad: indent\n---\n",
            ),
            (
                "two thematic breaks",
                "# Title\n\n---\n\nSection.\n\n---\n\nAnother.\n",
            ),
        ];

        // When
        let misplaced: Vec<Document> = cases
            .iter()
            .map(|(_, src)| Document::parse(src.as_bytes()))
            .collect();
        let unclosed = Document::parse(unclosed_candidate);
        let quiet_docs: Vec<Document> = quiet
            .iter()
            .map(|(_, src)| Document::parse(src.as_bytes()))
            .collect();

        // Then
        for ((name, _), d) in cases.iter().zip(misplaced) {
            let err = d.error.unwrap_or_else(|| panic!("{name}: want an error"));
            assert_eq!(err.kind, ErrKind::NotFirst, "{name}");
            assert_eq!(err.line, 3, "{name}");
        }

        let err = unclosed
            .error
            .expect("want an error after an unclosed candidate");
        assert_eq!(err.kind, ErrKind::NotFirst);
        assert_eq!(err.line, 7);

        for ((name, _), d) in quiet.iter().zip(quiet_docs) {
            assert!(
                d.error.is_none(),
                "{name}: unexpected {:?}",
                d.error.map(|e| e.msg)
            );
            assert!(!d.frontmatter.present, "{name}");
        }
    }

    #[test]
    fn unterminated_frontmatter_excludes_the_opening_fence() {
        // Given
        let src = b"---\nname: open-ended\nbody without a closing fence\n";

        // When
        let d = Document::parse(src);

        // Then
        assert_eq!(d.error.map(|e| e.kind), Some(ErrKind::Unterminated));
        assert!(!d.body.starts_with("---"), "body = {:?}", d.body);
        assert!(
            d.body.starts_with("name: open-ended"),
            "body = {:?}",
            d.body
        );
        assert!(d.body.contains("body without a closing fence"));
    }

    #[test]
    fn string_slice_variants() {
        // Given
        let cases: &[(&str, &str, Option<&[&str]>)] = &[
            (
                "sequence",
                "---\nallowed-tools:\n  - Read\n  - Bash\n---\n",
                Some(&["Read", "Bash"]),
            ),
            (
                "comma separated",
                "---\nallowed-tools: Read, Bash\n---\n",
                Some(&["Read", "Bash"]),
            ),
            (
                "flow sequence",
                "---\nallowed-tools: [Read, Bash]\n---\n",
                Some(&["Read", "Bash"]),
            ),
            (
                "mapping is rejected",
                "---\nallowed-tools:\n  read: true\n---\n",
                None,
            ),
            (
                "nested sequence is rejected",
                "---\nallowed-tools:\n  - [Read]\n---\n",
                None,
            ),
        ];
        for (name, src, want) in cases {
            let d = Document::parse(src.as_bytes());
            let (fm, _, err) = (d.frontmatter, d.body, d.error);
            assert!(err.is_none(), "{name}: {:?}", err.map(|e| e.msg));

            // When
            let got = fm.string_slice("allowed-tools");

            // Then
            match want {
                Some(want) => {
                    let got = got.unwrap_or_else(|| panic!("{name}: want Some, got None"));
                    assert_eq!(got, *want, "{name}");
                }
                None => assert!(got.is_none(), "{name}: got {got:?}"),
            }
        }
    }

    #[test]
    fn accessors_on_absent_keys() {
        // Given
        let d = Document::parse(b"---\nname: only-name\n---\n");
        let (fm, _, err) = (d.frontmatter, d.body, d.error);

        // When
        let has = fm.has("license");
        let node = fm.node("license");
        let string = fm.string("license");
        let line = fm.line("license");

        // Then
        assert!(err.is_none());
        assert!(!has);
        assert!(node.is_none());
        assert!(string.is_none());
        assert_eq!(line, fm.end_line);
    }

    #[test]
    fn node_kind() {
        // Given
        let d = Document::parse(b"---\nmetadata:\n  owner: infra\n---\n");
        let (fm, _, err) = (d.frontmatter, d.body, d.error);

        // When
        let node = fm.node("metadata");

        // Then
        assert!(err.is_none());
        assert!(matches!(node, Some(Value::Mapping)));
    }

    #[test]
    fn unsafe_yaml_refuses_alias_bombs_and_deep_nesting() {
        // Given
        let bomb = "a: &a [x]\nb: &b [*a,*a,*a]\nc: [*b,*b,*b]\n";
        let deep = format!("k: {}{}\n", "[".repeat(40), "]".repeat(40));
        let big = format!("k: {}\n", "a".repeat(70_000));
        let fine = "name: x\ndescription: |\n  * a bullet\n  and *.md globs\nlist: [a, b]\n";

        // When / Then
        assert!(unsafe_yaml(bomb).is_some());
        assert!(unsafe_yaml(&deep).is_some());
        assert!(unsafe_yaml(&big).is_some());
        assert!(unsafe_yaml(fine).is_none());
    }
}
