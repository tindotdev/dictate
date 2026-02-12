//! Persistent dictionary for Whisper prompt injection.
//!
//! Maps mis-transcribed words to their preferred spellings. Entries are
//! injected into Whisper's `prompt` parameter to bias token predictions
//! at transcription time.

pub mod error;
pub mod store;

pub use error::DictionaryError;
pub use store::DictionaryStore;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::token::estimate_token_count;

/// A dictionary mapping mis-transcribed words to their preferred spellings.
///
/// Uses [`BTreeMap`] for deterministic key ordering in serialized output
/// and table display.
///
/// # Invariants
///
/// - Keys are non-empty and trimmed.
/// - Values are non-empty and trimmed.
///
/// These are enforced by the [`insert`](Dictionary::insert) method; there is no
/// raw public field access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Dictionary {
    entries: BTreeMap<String, String>,
}

/// Result of formatting dictionary entries within a token budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptHint {
    /// The formatted hint string (comma-separated preferred spellings).
    pub text: String,
    /// Number of entries that fit within the budget.
    pub included: usize,
    /// Total number of entries in the dictionary.
    pub total: usize,
}

impl Dictionary {
    /// Create an empty dictionary.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Insert or update an entry.
    ///
    /// Both key and value are trimmed. Returns `None` if key or value is
    /// empty after trimming (the entry is **not** inserted). Otherwise returns
    /// the previous value if the key already existed.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        let key = key.into().trim().to_string();
        let value = value.into().trim().to_string();

        if key.is_empty() || value.is_empty() {
            return None;
        }

        self.entries.insert(key, value)
    }

    /// Look up the preferred replacement for a key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Remove an entry by key. Returns the removed value.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    /// Returns `true` if the dictionary has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over all entries in key-sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Format dictionary entries as a Whisper prompt hint.
    ///
    /// Returns a comma-separated list of **preferred spellings** (values only).
    /// The heard/mis-transcribed words (keys) are not included — the model
    /// only needs to know the correct spellings to bias its token predictions.
    ///
    /// Returns `None` if the dictionary is empty.
    #[must_use]
    pub fn as_prompt_hint(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        let hint: String =
            self.entries
                .values()
                .enumerate()
                .fold(String::new(), |mut acc, (i, v)| {
                    if i > 0 {
                        acc.push_str(", ");
                    }
                    acc.push_str(v);
                    acc
                });

        Some(hint)
    }

    /// Format dictionary entries as a prompt hint, fitting within a token budget.
    ///
    /// Includes as many preferred spellings as fit within `max_tokens`,
    /// iterating entries in key-sorted order. Returns `None` if the dictionary
    /// is empty or budget is zero.
    #[must_use]
    pub fn as_prompt_hint_within(&self, max_tokens: usize) -> Option<PromptHint> {
        if self.entries.is_empty() || max_tokens == 0 {
            return None;
        }

        let total = self.entries.len();
        let mut hint = String::new();
        let mut included = 0;

        for value in self.entries.values() {
            // Calculate what the string would look like with this entry appended
            let candidate = if hint.is_empty() {
                value.clone()
            } else {
                format!("{hint}, {value}")
            };

            if estimate_token_count(&candidate) > max_tokens {
                break;
            }

            hint = candidate;
            included += 1;
        }

        if included == 0 {
            return None;
        }

        Some(PromptHint {
            text: hint,
            included,
            total,
        })
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── insert / get / remove ─────────────────────────────────────────

    #[test]
    fn insert_and_get() {
        let mut dict = Dictionary::new();
        assert!(dict.insert("Cloud", "Claude").is_none());
        assert_eq!(dict.get("Cloud"), Some("Claude"));
    }

    #[test]
    fn insert_overwrites_and_returns_previous() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        let prev = dict.insert("Cloud", "Clod");
        assert_eq!(prev, Some("Claude".to_string()));
        assert_eq!(dict.get("Cloud"), Some("Clod"));
    }

    #[test]
    fn insert_trims_whitespace() {
        let mut dict = Dictionary::new();
        dict.insert("  Cloud  ", "  Claude  ");
        assert_eq!(dict.get("Cloud"), Some("Claude"));
        assert!(dict.get("  Cloud  ").is_none());
    }

    #[test]
    fn insert_rejects_empty_key() {
        let mut dict = Dictionary::new();
        assert!(dict.insert("", "Claude").is_none());
        assert!(dict.is_empty());
    }

    #[test]
    fn insert_rejects_whitespace_only_key() {
        let mut dict = Dictionary::new();
        assert!(dict.insert("   ", "Claude").is_none());
        assert!(dict.is_empty());
    }

    #[test]
    fn insert_rejects_empty_value() {
        let mut dict = Dictionary::new();
        assert!(dict.insert("Cloud", "").is_none());
        assert!(dict.is_empty());
    }

    #[test]
    fn remove_existing_entry() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        assert_eq!(dict.remove("Cloud"), Some("Claude".to_string()));
        assert!(dict.get("Cloud").is_none());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut dict = Dictionary::new();
        assert!(dict.remove("Cloud").is_none());
    }

    // ── len / is_empty ────────────────────────────────────────────────

    #[test]
    fn empty_dictionary() {
        let dict = Dictionary::new();
        assert!(dict.is_empty());
        assert_eq!(dict.len(), 0);
    }

    #[test]
    fn len_tracks_entries() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");
        assert_eq!(dict.len(), 2);
        assert!(!dict.is_empty());
    }

    // ── iteration order ───────────────────────────────────────────────

    #[test]
    fn iter_returns_sorted_by_key() {
        let mut dict = Dictionary::new();
        dict.insert("Tim", "Tin");
        dict.insert("Cloud", "Claude");
        dict.insert("coop", "Co-op");

        let keys: Vec<&str> = dict.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["Cloud", "Tim", "coop"]);
    }

    // ── as_prompt_hint ────────────────────────────────────────────────

    #[test]
    fn prompt_hint_empty_dictionary() {
        let dict = Dictionary::new();
        assert!(dict.as_prompt_hint().is_none());
    }

    #[test]
    fn prompt_hint_single_entry() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        assert_eq!(dict.as_prompt_hint(), Some("Claude".to_string()));
    }

    #[test]
    fn prompt_hint_multiple_entries_comma_separated() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");
        // BTreeMap sorts by key: Cloud < Tim
        assert_eq!(dict.as_prompt_hint(), Some("Claude, Tin".to_string()));
    }

    #[test]
    fn prompt_hint_values_only() {
        let mut dict = Dictionary::new();
        dict.insert("wrong", "correct");
        let hint = dict.as_prompt_hint().unwrap();
        assert!(!hint.contains("wrong"));
        assert!(hint.contains("correct"));
    }

    // ── as_prompt_hint_within ─────────────────────────────────────────

    #[test]
    fn prompt_hint_within_empty_dictionary() {
        let dict = Dictionary::new();
        assert!(dict.as_prompt_hint_within(100).is_none());
    }

    #[test]
    fn prompt_hint_within_zero_budget() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        assert!(dict.as_prompt_hint_within(0).is_none());
    }

    #[test]
    fn prompt_hint_within_all_fit() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");

        let hint = dict.as_prompt_hint_within(100).unwrap();
        assert_eq!(hint.text, "Claude, Tin");
        assert_eq!(hint.included, 2);
        assert_eq!(hint.total, 2);
    }

    #[test]
    fn prompt_hint_within_truncates() {
        let mut dict = Dictionary::new();
        dict.insert("A", "Alpha");
        dict.insert("B", "Bravo");
        dict.insert("C", "Charlie");

        // "Alpha" = 5 tokens, "Alpha, Bravo" = 12 tokens
        // Budget of 10 should include only "Alpha"
        let hint = dict.as_prompt_hint_within(10).unwrap();
        assert_eq!(hint.text, "Alpha");
        assert_eq!(hint.included, 1);
        assert_eq!(hint.total, 3);
    }

    #[test]
    fn prompt_hint_within_budget_too_small_for_first_entry() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");

        // "Claude" = 6 tokens, budget is 3
        assert!(dict.as_prompt_hint_within(3).is_none());
    }

    #[test]
    fn prompt_hint_within_exact_fit() {
        let mut dict = Dictionary::new();
        dict.insert("A", "abc");

        // "abc" = 3 tokens, budget is exactly 3
        let hint = dict.as_prompt_hint_within(3).unwrap();
        assert_eq!(hint.text, "abc");
        assert_eq!(hint.included, 1);
        assert_eq!(hint.total, 1);
    }

    // ── serde round-trip ──────────────────────────────────────────────

    #[test]
    fn serde_round_trip() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");

        let json = serde_json::to_string_pretty(&dict).unwrap();
        let deserialized: Dictionary = serde_json::from_str(&json).unwrap();
        assert_eq!(dict, deserialized);
    }

    #[test]
    fn serde_flat_json_format() {
        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");

        let json = serde_json::to_string(&dict).unwrap();
        assert_eq!(json, r#"{"Cloud":"Claude"}"#);
    }

    #[test]
    fn deserialize_from_flat_json() {
        let json = r#"{"Cloud": "Claude", "Tim": "Tin"}"#;
        let dict: Dictionary = serde_json::from_str(json).unwrap();
        assert_eq!(dict.get("Cloud"), Some("Claude"));
        assert_eq!(dict.get("Tim"), Some("Tin"));
        assert_eq!(dict.len(), 2);
    }
}
