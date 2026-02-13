//! Prompt hint composition utilities.
//!
//! Merges dictionary correction values and vocabulary words into a unified,
//! deduplicated hint set and formats a budgeted prompt hint string.

use std::collections::BTreeSet;

use crate::token::estimate_token_count;
use crate::{Dictionary, Vocabulary};

/// Result of formatting prompt hints within a token budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptHint {
    /// The formatted hint string (comma-separated entries).
    pub text: String,
    /// Number of entries that fit within the budget.
    pub included: usize,
    /// Total number of entries available.
    pub total: usize,
}

/// Merge dictionary values and vocabulary words into a deduplicated,
/// alphabetically sorted set of prompt hints.
#[must_use]
pub fn merge_prompt_hints(dictionary: &Dictionary, vocabulary: &Vocabulary) -> BTreeSet<String> {
    let mut hints = BTreeSet::new();

    for (_, preferred) in dictionary.iter() {
        hints.insert(preferred.to_string());
    }

    for word in vocabulary.iter() {
        hints.insert(word.to_string());
    }

    hints
}

/// Format prompt hints as a comma-separated list within `max_tokens`.
///
/// Includes as many hints as fit, in the iterator's existing order.
/// Returns `None` if there are no hints, `max_tokens` is zero, or the first
/// hint alone exceeds the budget.
#[must_use]
pub fn format_hint_within_budget<'a, I>(hints: I, max_tokens: usize) -> Option<PromptHint>
where
    I: IntoIterator<Item = &'a str>,
{
    if max_tokens == 0 {
        return None;
    }

    let entries: Vec<&str> = hints.into_iter().collect();
    if entries.is_empty() {
        return None;
    }

    let total = entries.len();
    let mut text = String::new();
    let mut included = 0;

    for entry in entries {
        let candidate = if text.is_empty() {
            entry.to_string()
        } else {
            format!("{text}, {entry}")
        };

        if estimate_token_count(&candidate) > max_tokens {
            break;
        }

        text = candidate;
        included += 1;
    }

    if included == 0 {
        return None;
    }

    Some(PromptHint {
        text,
        included,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prompt_hints_empty_sources() {
        let dict = Dictionary::new();
        let vocab = Vocabulary::new();

        let hints = merge_prompt_hints(&dict, &vocab);
        assert!(hints.is_empty());
    }

    #[test]
    fn merge_prompt_hints_dictionary_only() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");
        let vocab = Vocabulary::new();

        let hints = merge_prompt_hints(&dict, &vocab);
        let values: Vec<&str> = hints.iter().map(String::as_str).collect();
        assert_eq!(values, vec!["Claude", "Tin"]);
    }

    #[test]
    fn merge_prompt_hints_vocab_only() {
        let dict = Dictionary::new();
        let mut vocab = Vocabulary::new();
        vocab.insert("OpenAI");
        vocab.insert("AWS");

        let hints = merge_prompt_hints(&dict, &vocab);
        let values: Vec<&str> = hints.iter().map(String::as_str).collect();
        assert_eq!(values, vec!["AWS", "OpenAI"]);
    }

    #[test]
    fn merge_prompt_hints_deduplicates_overlap() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Team", "Tin");

        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        vocab.insert("Claude");
        vocab.insert("Kubernetes");

        let hints = merge_prompt_hints(&dict, &vocab);
        let values: Vec<&str> = hints.iter().map(String::as_str).collect();
        assert_eq!(values, vec!["AWS", "Claude", "Kubernetes", "Tin"]);
    }

    #[test]
    fn format_hint_within_budget_empty_input() {
        let hints: Vec<&str> = vec![];
        assert!(format_hint_within_budget(hints, 10).is_none());
    }

    #[test]
    fn format_hint_within_budget_zero_budget() {
        let hints = ["AWS", "OpenAI"];
        assert!(format_hint_within_budget(hints, 0).is_none());
    }

    #[test]
    fn format_hint_within_budget_all_fit() {
        let hints = ["AWS", "OpenAI"];
        let hint = format_hint_within_budget(hints, 100).unwrap();

        assert_eq!(hint.text, "AWS, OpenAI");
        assert_eq!(hint.included, 2);
        assert_eq!(hint.total, 2);
    }

    #[test]
    fn format_hint_within_budget_truncates_in_order() {
        let hints = ["Alpha", "Bravo", "Charlie"];
        let hint = format_hint_within_budget(hints, 10).unwrap();

        assert_eq!(hint.text, "Alpha");
        assert_eq!(hint.included, 1);
        assert_eq!(hint.total, 3);
    }

    #[test]
    fn format_hint_within_budget_first_entry_too_large() {
        let hints = ["OpenAI"];
        assert!(format_hint_within_budget(hints, 3).is_none());
    }

    #[test]
    fn format_hint_within_budget_exact_fit() {
        let hints = ["abc"];
        let hint = format_hint_within_budget(hints, 3).unwrap();

        assert_eq!(hint.text, "abc");
        assert_eq!(hint.included, 1);
        assert_eq!(hint.total, 1);
    }
}
