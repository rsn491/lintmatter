//! Small text helpers for rendering reports.

/// Formats a count with the right noun, as "1 file" or "2 files".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_picks_the_noun() {
        assert_eq!(plural(0, "file", "files"), "0 files");
        assert_eq!(plural(1, "file", "files"), "1 file");
        assert_eq!(plural(2, "file", "files"), "2 files");
    }
}
