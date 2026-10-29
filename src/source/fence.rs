//! Tracks fenced code blocks so their contents can be excluded from checks
//! that only make sense against prose.

/// Tracks which lines sit inside a fenced code block, so their contents are
/// not scanned for file references.
///
/// A single boolean is not enough: a fence closes only on a run of the same
/// marker character at least as long as the one that opened it, so a ``` line
/// inside a ```` block is content rather than a close. Getting that wrong
/// re-exposes the block body to the reference check, and
/// `file-reference.missing` is an error, so it fails a build on a correct file.
#[derive(Default)]
pub struct FenceTracker {
    /// The open fence's marker character and run length, `None` outside a block.
    open: Option<(char, usize)>,
}

impl FenceTracker {
    /// Feeds the tracker one line and reports whether that line's content
    /// should be scanned. Fence lines themselves never are.
    pub fn scan_line(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        let Some((marker, len, info)) = fence_parts(trimmed) else {
            return self.open.is_none();
        };
        match self.open {
            // Inside a block, only a matching fence closes it: same marker, at
            // least as long, and no info string. Anything else -- a shorter
            // run, the other marker, ```rust -- is block content.
            Some((open_marker, open_len)) => {
                if marker == open_marker && len >= open_len && info.trim().is_empty() {
                    self.open = None;
                }
            }
            None => self.open = Some((marker, len)),
        }
        false
    }
}

/// Splits a fence line into its marker character, the length of its marker run,
/// and the info string that follows. `None` when the line is not a fence.
fn fence_parts(trimmed: &str) -> Option<(char, usize, &str)> {
    let marker = match trimmed.chars().next()? {
        c @ ('`' | '~') => c,
        _ => return None,
    };
    // Markers are ASCII, so the char count doubles as a byte offset.
    let len = trimmed.chars().take_while(|c| *c == marker).count();
    if len < 3 {
        return None;
    }
    Some((marker, len, &trimmed[len..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_matching_fence_close() {
        let mut t = FenceTracker::default();
        assert!(!t.scan_line("```"));
        assert!(!t.scan_line("inside"));
        assert!(!t.scan_line("```"));
        assert!(t.scan_line("outside"));
    }

    #[test]
    fn shorter_or_differing_marker_does_not_close() {
        let mut t = FenceTracker::default();
        assert!(!t.scan_line("````"));
        assert!(!t.scan_line("```"));
        assert!(!t.scan_line("~~~"));
        assert!(!t.scan_line("````"));
        assert!(t.scan_line("outside"));
    }

    #[test]
    fn info_string_does_not_close() {
        let mut t = FenceTracker::default();
        assert!(!t.scan_line("```"));
        assert!(!t.scan_line("```rust"));
        assert!(!t.scan_line("inside"));
        assert!(!t.scan_line("```"));
        assert!(t.scan_line("outside"));
    }
}
