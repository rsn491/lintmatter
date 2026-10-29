//! Totals for a whole run.

use crate::lint::FileResult;

/// Counts a whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub files: usize,
    pub files_with_errors: usize,
    pub files_with_warnings: usize,
    /// The mean of the per-file scores, so the run's rating is exactly the
    /// average of the numbers printed above it. An empty run rates 100.
    pub score: u8,
}

/// Counts, across results, how many files have at least one error or
/// warning. Mirrors other linters' summaries, which report the files
/// affected rather than a raw finding tally.
pub fn summarize(results: &[FileResult]) -> Summary {
    let mut s = Summary {
        files: results.len(),
        ..Default::default()
    };
    let mut total: u32 = 0;
    for r in results {
        if r.errors() > 0 {
            s.files_with_errors += 1;
        }
        if r.warnings() > 0 {
            s.files_with_warnings += 1;
        }
        total += u32::from(r.score);
    }
    s.score = if results.is_empty() {
        100
    } else {
        // Floored, matching the per-file scores it averages.
        (total / results.len() as u32) as u8
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::results;

    #[test]
    fn summarize_totals_across_results() {
        assert_eq!(
            summarize(&results()),
            Summary {
                files: 2,
                files_with_errors: 1,
                files_with_warnings: 1,
                score: 73,
            }
        );
    }
}
