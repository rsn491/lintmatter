//! Estimates how many tokens a piece of text costs a language model, without
//! shipping a tokenizer vocabulary.

use crate::utils::ceil_div;

/// Reports the token cost of a string. The heuristic [`Estimator`] is the only
/// implementation today; the trait is the seam where a real BPE tokenizer
/// could be substituted without touching the lint rules.
pub trait Counter {
    fn count(&self, s: &str) -> usize;
}

/// Approximates byte-pair encoding by measuring the shape of the text rather
/// than looking words up in a vocabulary.
///
/// Accuracy: on English prose it lands within roughly 15% of cl100k_base and
/// o200k_base. On source code, punctuation-dense text and non-Latin scripts it
/// deliberately errs high, so a budget that passes here also passes against a
/// real tokenizer.
///
/// The rules, applied in a single pass:
///
///   - A run of n ASCII letters or digits costs ceil(n/4) tokens, at least 1.
///     Real BPE merges common short words into one token and splits long ones
///     into word pieces of roughly four characters.
///   - Each punctuation or symbol rune costs 1 token.
///   - A single space between chunks is free: BPE folds the leading space into
///     the token that follows it. Each newline costs 1 token, and a whitespace
///     run of n characters beyond the first costs ceil(n/4) (indentation and
///     blank-line padding are cheap but not free).
///   - Each non-ASCII rune costs 1 token. CJK, emoji and other scripts tokenize
///     far denser than Latin text, often at or above one token per character.
pub struct Estimator;

impl Estimator {
    pub fn new() -> Self {
        Estimator
    }
}

impl Default for Estimator {
    fn default() -> Self {
        Self::new()
    }
}

impl Counter for Estimator {
    fn count(&self, s: &str) -> usize {
        // (total, consecutive word runes, consecutive non-newline whitespace)
        let flush_word =
            |(total, runs, space): (usize, usize, usize)| (total + ceil_div(runs, 4), 0, space);
        let flush_space = |(total, runs, space): (usize, usize, usize)| {
            let extra = if space > 1 { ceil_div(space - 1, 4) } else { 0 };
            (total + extra, runs, 0)
        };

        let (total, runs, space) = s.chars().fold((0, 0, 0), |state, r| {
            if r == '\n' || r == '\r' || r as u32 > 127 {
                let (total, runs, space) = flush_space(flush_word(state));
                (total + 1, runs, space)
            } else if r.is_whitespace() {
                let (total, runs, space) = flush_word(state);
                (total, runs, space + 1)
            } else if is_word_rune(r) {
                let (total, runs, space) = flush_space(state);
                (total, runs + 1, space)
            } else {
                let (total, runs, space) = flush_space(flush_word(state));
                (total + 1, runs, space)
            }
        });
        flush_space(flush_word((total, runs, space))).0
    }
}

fn is_word_rune(r: char) -> bool {
    r.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_matches_reference_cases() {
        // Given
        let cases: &[(&str, usize)] = &[
            ("", 0),
            ("hello", 2),
            ("a", 1),
            ("hello world", 4),
            ("internationalization", 5),
            ("a, b.", 4),
            ("a\nb", 3),
            ("a\n    b", 4),
            ("日本語", 3),
            ("🙂🙂", 2),
            ("func main() {", 5),
        ];
        let e = Estimator::new();
        for (input, want) in cases {
            // When
            let got = e.count(input);

            // Then
            assert_eq!(got, *want, "count({input:?})");
        }
    }

    #[test]
    fn prose_ratio_is_reasonable() {
        // Given
        let prose = "The linter reads every instruction file in the repository and \
            reports the ones that have grown past their token budget, so a reviewer \
            can see the cost of the change before it lands on the default branch.";
        let e = Estimator::new();

        // When
        let got = e.count(prose);

        // Then
        let ratio = prose.len() as f64 / got as f64;
        assert!(
            (3.2..=5.0).contains(&ratio),
            "count = {got} tokens for {} chars ({ratio:.2} chars/token), want [3.2, 5.0]",
            prose.len()
        );
    }

    #[test]
    fn count_is_monotonic() {
        // Given
        let e = Estimator::new();
        let mut text = String::new();

        // When
        let mut counts = Vec::new();
        for _ in 0..50 {
            text.push_str("another sentence of filler prose. ");
            counts.push(e.count(&text));
        }

        // Then
        for pair in counts.windows(2) {
            let (prev, got) = (pair[0], pair[1]);
            assert!(got >= prev, "count dropped from {prev} to {got}");
        }
    }
}
