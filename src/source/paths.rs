//! Lexical path helpers, so a path renders and compares the same way on
//! every platform without touching the filesystem.

use std::path::{Component, Path, PathBuf};

/// Renders a path with forward slashes, so output is identical on Windows.
pub fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Lexically normalizes a path: resolves `.` and `..` textually, without
/// touching the filesystem or following symlinks (mirroring Go's
/// `filepath.Clean`). `..` above the root is dropped rather than escaping it.
pub fn clean_path(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
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
    }
    let mut result = PathBuf::new();
    for c in out {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_normalizes_lexically() {
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
            assert_eq!(clean_path(Path::new(input)), PathBuf::from(want), "{input}");
        }
    }
}
