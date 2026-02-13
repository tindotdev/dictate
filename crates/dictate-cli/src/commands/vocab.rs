//! `dictate vocab` command family.

use dictate_core::{DictionaryError, DictionaryStore, VocabularyError, VocabularyStore};
use thiserror::Error;

use crate::args::{VocabArgs, VocabCommand};

/// Errors from the `vocab` command.
#[derive(Debug, Error)]
pub enum VocabError {
    /// Vocabulary store error (load/save/path resolution).
    #[error(transparent)]
    Vocabulary(#[from] VocabularyError),

    /// Dictionary store error (for cross-system dedup on `vocab add`).
    #[error(transparent)]
    Dictionary(#[from] DictionaryError),
}

/// Run the `vocab` command family.
pub fn run(args: &VocabArgs) -> Result<(), VocabError> {
    match &args.command {
        VocabCommand::Add { words } => run_add(words),
        VocabCommand::Remove { words } => run_remove(words),
        VocabCommand::List => run_list(),
    }
}

fn run_add(words: &[String]) -> Result<(), VocabError> {
    let vocab_store = VocabularyStore::open()?;
    let mut vocab = vocab_store.load()?;

    let dict_store = DictionaryStore::open()?;
    let dict = dict_store.load()?;

    let mut added = 0_usize;

    for raw_word in words {
        let word = raw_word.trim();
        if word.is_empty() {
            eprintln!("  ⚠ Skipped empty word");
            continue;
        }

        if vocab.contains(word) {
            eprintln!("  ⚠ Already in vocabulary, skipped: {word}");
            continue;
        }

        if let Some((heard, preferred)) = dict.iter().find(|(_, preferred)| *preferred == word) {
            eprintln!(
                "  ⚠ Already a dictionary correction ({heard} → {preferred}), skipped: {word}"
            );
            continue;
        }

        if vocab.insert(word) {
            added += 1;
            eprintln!("  ✓ Added: {word}");
        }
    }

    if added > 0 {
        vocab_store.save(&vocab)?;
    }

    eprintln!("Vocabulary: {} words", vocab.len());
    Ok(())
}

fn run_remove(words: &[String]) -> Result<(), VocabError> {
    let vocab_store = VocabularyStore::open()?;
    let mut vocab = vocab_store.load()?;

    let mut removed = 0_usize;

    for raw_word in words {
        let word = raw_word.trim();
        if word.is_empty() {
            eprintln!("  ⚠ Skipped empty word");
            continue;
        }

        if vocab.remove(word) {
            removed += 1;
            eprintln!("  ✓ Removed: {word}");
        } else {
            eprintln!("  ⚠ Not in vocabulary: {word}");
        }
    }

    if removed > 0 {
        vocab_store.save(&vocab)?;
    }

    eprintln!("Vocabulary: {} words", vocab.len());
    Ok(())
}

fn run_list() -> Result<(), VocabError> {
    let vocab_store = VocabularyStore::open()?;
    let vocab = vocab_store.load()?;

    if vocab.is_empty() {
        eprintln!("Vocabulary is empty. Use `dictate vocab add <word>` to add words.");
        return Ok(());
    }

    eprintln!("Vocabulary ({} words):", vocab.len());

    let words: Vec<&str> = vocab.iter().collect();
    println!("  {}", words.join(", "));

    if let Some(dictionary_count) = load_dictionary_count_best_effort() {
        eprintln!();
        eprintln!(
            "Also: {} dictionary {} (use `dictate dictionary` to view)",
            dictionary_count,
            if dictionary_count == 1 {
                "correction"
            } else {
                "corrections"
            }
        );
    }

    Ok(())
}

fn load_dictionary_count_best_effort() -> Option<usize> {
    let store = DictionaryStore::open().ok()?;
    let dict = store.load().ok()?;

    if dict.is_empty() {
        None
    } else {
        Some(dict.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_count_best_effort_handles_missing_store() {
        // Open/load behavior depends on platform env and filesystem state.
        // This test only asserts that the best-effort helper never panics.
        let _ = load_dictionary_count_best_effort();
    }
}
