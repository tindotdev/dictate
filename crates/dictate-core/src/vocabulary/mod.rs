//! Persistent vocabulary for Whisper prompt injection.
//!
//! Stores words that should be recognized as-is (e.g., product names,
//! acronyms, and technical jargon) and injected into Whisper's `prompt`
//! parameter at transcription time.

pub mod error;
pub mod store;

pub use error::VocabularyError;
pub use store::VocabularyStore;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A sorted set of vocabulary words for Whisper transcription biasing.
///
/// Uses [`BTreeSet`] for deterministic sorted ordering in serialized output
/// and display.
///
/// # Invariants
///
/// - Words are non-empty and trimmed.
/// - Duplicate words are not stored.
///
/// These are enforced by the [`insert`](Vocabulary::insert) method; there is no
/// raw public field access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vocabulary {
    words: BTreeSet<String>,
}

impl Vocabulary {
    /// Create an empty vocabulary.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            words: BTreeSet::new(),
        }
    }

    /// Add a word.
    ///
    /// The word is trimmed. Returns `false` if it is empty after trimming
    /// or already present.
    pub fn insert(&mut self, word: impl Into<String>) -> bool {
        let word = word.into().trim().to_string();
        if word.is_empty() {
            return false;
        }

        self.words.insert(word)
    }

    /// Remove a word. Returns `true` if the word was present.
    pub fn remove(&mut self, word: &str) -> bool {
        self.words.remove(word)
    }

    /// Check if a word is in the vocabulary.
    #[must_use]
    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(word)
    }

    /// Returns `true` if the vocabulary has no words.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Number of words.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Iterate over all words in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.words.iter().map(String::as_str)
    }
}

impl Default for Vocabulary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_contains() {
        let mut vocab = Vocabulary::new();
        assert!(vocab.insert("AWS"));
        assert!(vocab.contains("AWS"));
    }

    #[test]
    fn insert_trims_whitespace() {
        let mut vocab = Vocabulary::new();
        assert!(vocab.insert("  OpenAI  "));
        assert!(vocab.contains("OpenAI"));
        assert!(!vocab.contains("  OpenAI  "));
    }

    #[test]
    fn insert_rejects_empty() {
        let mut vocab = Vocabulary::new();
        assert!(!vocab.insert(""));
        assert!(vocab.is_empty());
    }

    #[test]
    fn insert_rejects_whitespace_only() {
        let mut vocab = Vocabulary::new();
        assert!(!vocab.insert("   "));
        assert!(vocab.is_empty());
    }

    #[test]
    fn duplicate_insert_returns_false() {
        let mut vocab = Vocabulary::new();
        assert!(vocab.insert("kubectl"));
        assert!(!vocab.insert("kubectl"));
        assert_eq!(vocab.len(), 1);
    }

    #[test]
    fn remove_existing_word() {
        let mut vocab = Vocabulary::new();
        vocab.insert("gRPC");
        assert!(vocab.remove("gRPC"));
        assert!(!vocab.contains("gRPC"));
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut vocab = Vocabulary::new();
        assert!(!vocab.remove("Terraform"));
        assert!(vocab.is_empty());
    }

    #[test]
    fn remove_requires_exact_match() {
        let mut vocab = Vocabulary::new();
        vocab.insert("Kubernetes");
        assert!(!vocab.remove("  Kubernetes  "));
        assert!(vocab.remove("Kubernetes"));
    }

    #[test]
    fn len_tracks_words() {
        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        vocab.insert("OpenAI");
        assert_eq!(vocab.len(), 2);
        assert!(!vocab.is_empty());
    }

    #[test]
    fn iter_returns_sorted_words() {
        let mut vocab = Vocabulary::new();
        vocab.insert("kubectl");
        vocab.insert("AWS");
        vocab.insert("gRPC");

        let words: Vec<&str> = vocab.iter().collect();
        assert_eq!(words, vec!["AWS", "gRPC", "kubectl"]);
    }

    #[test]
    fn serde_round_trip() {
        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        vocab.insert("OpenAI");

        let json = serde_json::to_string_pretty(&vocab).unwrap();
        let deserialized: Vocabulary = serde_json::from_str(&json).unwrap();
        assert_eq!(vocab, deserialized);
    }

    #[test]
    fn serde_json_array_format() {
        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        vocab.insert("OpenAI");

        let json = serde_json::to_string(&vocab).unwrap();
        assert_eq!(json, r#"["AWS","OpenAI"]"#);
    }

    #[test]
    fn deserialize_from_json_array() {
        let json = r#"["AWS", "OpenAI", "kubectl"]"#;
        let vocab: Vocabulary = serde_json::from_str(json).unwrap();
        assert_eq!(vocab.len(), 3);
        assert!(vocab.contains("AWS"));
        assert!(vocab.contains("OpenAI"));
        assert!(vocab.contains("kubectl"));
    }
}
