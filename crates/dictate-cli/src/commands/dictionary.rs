//! `dictate dictionary` command — print dictionary entries as a table.

use dictate_core::{DictionaryError, DictionaryStore};
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

    Ok(())
}
