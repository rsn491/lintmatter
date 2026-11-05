use crate::domain::FileResult;

/// Counts a whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    let total: u32 = results.iter().map(|r| u32::from(r.score)).sum();
    Summary {
        files: results.len(),
        files_with_errors: results.iter().filter(|r| r.errors() > 0).count(),
        files_with_warnings: results.iter().filter(|r| r.warnings() > 0).count(),
        score: if results.is_empty() {
            100
        } else {
            // Floored, matching the per-file scores it averages.
            (total / results.len() as u32) as u8
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_utils::results;

    #[test]
    fn summarize_totals_across_results() {
        // Given
        let results = results();

        // When
        let got = summarize(&results);

        // Then
        assert_eq!(
            got,
            Summary {
                files: 2,
                files_with_errors: 1,
                files_with_warnings: 1,
                score: 73,
            }
        );
    }
}
