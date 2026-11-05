//! Small, general-purpose helpers shared across modules.
//!
//! Everything here is a pure function of its arguments: no I/O, no global
//! state, nothing specific to linting. Anything that needs to know what a rule
//! or a report is belongs in its own module instead.

use std::path::{Component, Path, PathBuf};

/// Renders n with thousands separators, so "6,142 tokens" reads at a glance
/// in reports.
pub fn humanize(n: i64) -> String {
    let s = n.to_string();
    if n < 0 {
        return s;
    }
    let bytes = s.as_bytes();
    let len = bytes.len();
    bytes
        .iter()
        .enumerate()
        .flat_map(|(i, b)| {
            let comma = (i > 0 && (len - i).is_multiple_of(3)).then_some(',');
            comma.into_iter().chain(std::iter::once(*b as char))
        })
        .collect()
}

/// Formats a count with the right noun, as "1 file" or "2 files".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Renders a path with forward slashes, so output is identical on Windows.
pub fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Makes control characters visible (`\u{1b}`), so a name taken from disk
/// cannot move the cursor, recolor the terminal or forge a report line.
pub fn escape_control(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            let escaped: Vec<char> = if c.is_control() {
                c.escape_default().collect()
            } else {
                vec![c]
            };
            escaped
        })
        .collect()
}

/// Lexically normalizes a path: resolves `.` and `..` textually, without
/// touching the filesystem or following symlinks (mirroring Go's
/// `filepath.Clean`). `..` above the root is dropped rather than escaping it.
pub fn clean_path(path: &Path) -> PathBuf {
    let out = path
        .components()
        .fold(Vec::<Component>::new(), |mut out, comp| {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => match out.last() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    Some(Component::RootDir) => {}
                    _ => out.push(comp),
                },
                other => out.push(other),
            }
            out
        });
    let result: PathBuf = out.iter().map(|c| c.as_os_str()).collect();
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

/// Divides, rounding up. Zero over anything is zero.
pub fn ceil_div(a: usize, b: usize) -> usize {
    if a == 0 { 0 } else { a.div_ceil(b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_formats_with_separators() {
        // Given
        let cases: &[(i64, &str)] = &[
            (0, "0"),
            (7, "7"),
            (999, "999"),
            (1000, "1,000"),
            (6142, "6,142"),
            (1_234_567, "1,234,567"),
            (-42, "-42"),
        ];
        for (n, want) in cases {
            // When
            let got = humanize(*n);

            // Then
            assert_eq!(got, *want, "humanize({n})");
        }
    }

    #[test]
    fn plural_picks_the_noun() {
        // Given
        let counts = [0, 1, 2];

        // When
        let got = counts.map(|n| plural(n, "file", "files"));

        // Then
        assert_eq!(got, ["0 files", "1 file", "2 files"]);
    }

    #[test]
    fn clean_path_normalizes_lexically() {
        // Given
        let cases: &[(&str, &str)] = &[
            ("a/./b", "a/b"),
            ("a/b/../c", "a/c"),
            ("./a", "a"),
            (".", "."),
            ("", "."),
            ("/a/../../b", "/b"),
            ("../a", "../a"),
        ];
        for (input, want) in cases {
            // When
            let got = clean_path(Path::new(input));

            // Then
            assert_eq!(got, PathBuf::from(want), "{input}");
        }
    }

    #[test]
    fn ceil_div_rounds_up() {
        // Given
        let numerators = [0, 1, 4, 5];

        // When
        let got = numerators.map(|a| ceil_div(a, 4));

        // Then
        assert_eq!(got, [0, 1, 1, 2]);
    }

    #[test]
    fn escape_control_makes_terminal_codes_visible() {
        // Given
        let hostile = "a\u{1b}[31m\nb";

        // When
        let out = escape_control(hostile);

        // Then
        assert_eq!(out, "a\\u{1b}[31m\\nb");
        assert_eq!(escape_control("plain/päth"), "plain/päth");
    }
}
