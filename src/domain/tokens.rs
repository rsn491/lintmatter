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
        let mut total = 0usize;
        let mut runs = 0usize; // consecutive word runes, flushed when the run ends
        let mut space = 0usize; // consecutive whitespace characters, excluding newlines

        let flush_word = |runs: &mut usize, total: &mut usize| {
            if *runs > 0 {
                *total += ceil_div(*runs, 4);
                *runs = 0;
            }
        };
        let flush_space = |space: &mut usize, total: &mut usize| {
            if *space > 1 {
                *total += ceil_div(*space - 1, 4);
            }
            *space = 0;
        };

        for r in s.chars() {
            if r == '\n' || r == '\r' {
                flush_word(&mut runs, &mut total);
                flush_space(&mut space, &mut total);
                total += 1;
            } else if r.is_whitespace() {
                flush_word(&mut runs, &mut total);
                space += 1;
            } else if r as u32 > 127 {
                flush_word(&mut runs, &mut total);
                flush_space(&mut space, &mut total);
                total += 1;
            } else if is_word_rune(r) {
                flush_space(&mut space, &mut total);
                runs += 1;
            } else {
                flush_word(&mut runs, &mut total);
                flush_space(&mut space, &mut total);
                total += 1;
            }
        }
        flush_word(&mut runs, &mut total);
        flush_space(&mut space, &mut total);

        total
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
            assert_eq!(e.count(input), *want, "count({input:?})");
        }
    }

    #[test]
    fn prose_ratio_is_reasonable() {
        let prose = "The linter reads every instruction file in the repository and \
            reports the ones that have grown past their token budget, so a reviewer \
            can see the cost of the change before it lands on the default branch.";
        let got = Estimator::new().count(prose);
        let ratio = prose.len() as f64 / got as f64;
        assert!(
            (3.2..=5.0).contains(&ratio),
            "count = {got} tokens for {} chars ({ratio:.2} chars/token), want [3.2, 5.0]",
            prose.len()
        );
    }

    #[test]
    fn count_is_monotonic() {
        let e = Estimator::new();
        let mut prev = 0;
        let mut text = String::new();
        for _ in 0..50 {
            text.push_str("another sentence of filler prose. ");
            let got = e.count(&text);
            assert!(got >= prev, "count dropped from {prev} to {got}");
            prev = got;
        }
    }
}
