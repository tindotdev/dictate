//! `dictate dictionary` command — print dictionary entries as a table.

use dictate_core::{DictionaryError, DictionaryStore, VocabularyStore};
use tabled::{Table, Tabled, settings::Style};

#[derive(Tabled)]
struct DictionaryRow {
    #[tabled(rename = "Heard")]
    heard: String,
    #[tabled(rename = "Preferred")]
    preferred: String,
}

/// Print the current dictionary as a formatted table.
///
/// Header and empty-dictionary messages go to **stderr** (informational).
/// The table body goes to **stdout** (pipe-friendly).
pub fn run() -> Result<(), DictionaryError> {
    let store = DictionaryStore::open()?;
    let dict = store.load()?;

    if dict.is_empty() {
        eprintln!("Dictionary is empty. Use `dictate remember` to add entries.");
        return Ok(());
    }

    eprintln!(
        "Dictionary ({} {}):",
        dict.len(),
        if dict.len() == 1 { "entry" } else { "entries" }
    );
    eprintln!();

    let rows: Vec<DictionaryRow> = dict
        .iter()
        .map(|(heard, preferred)| DictionaryRow {
            heard: heard.to_string(),
            preferred: preferred.to_string(),
        })
        .collect();

    let table = Table::new(rows).with(Style::rounded()).to_string();

    println!("{table}");

    if let Some(vocabulary_count) = load_vocabulary_count_best_effort() {
        eprintln!();
        eprintln!(
            "Also: {} vocabulary {} (use `dictate vocab list` to view)",
            vocabulary_count,
            if vocabulary_count == 1 {
                "word"
            } else {
                "words"
            }
        );
    }

    Ok(())
}

fn load_vocabulary_count_best_effort() -> Option<usize> {
    let store = VocabularyStore::open().ok()?;
    let vocab = store.load().ok()?;

    if vocab.is_empty() {
        None
    } else {
        Some(vocab.len())
    }
}
