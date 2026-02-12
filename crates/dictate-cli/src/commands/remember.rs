//! Interactive `dictate remember` command.
//!
//! Prompts the user for a mis-transcribed word and its preferred spelling,
//! then persists the entry to the dictionary store.

use dictate_core::{DictionaryError, DictionaryStore};
use thiserror::Error;

/// Errors from the `remember` command.
#[derive(Debug, Error)]
pub enum RememberError {
    /// Dictionary store error (load/save/path resolution).
    #[error(transparent)]
    Dictionary(#[from] DictionaryError),

    /// Interactive prompt error (e.g., stdin closed).
    #[error("input error: {0}")]
    Input(#[from] dialoguer::Error),
}

/// Run the interactive `remember` flow.
///
/// 1. Prompt for "Heard" (mis-transcribed word) — non-empty, trimmed.
/// 2. Prompt for "Preferred" (correct spelling) — non-empty, trimmed.
/// 3. If key exists: show current vs new, confirm overwrite.
/// 4. Save to store.
pub fn run() -> Result<(), RememberError> {
    let store = DictionaryStore::open()?;
    let mut dict = store.load()?;

    let heard: String = dialoguer::Input::new()
        .with_prompt("Heard (mis-transcribed word)")
        .validate_with(|input: &String| {
            if input.trim().is_empty() {
                Err("cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    let heard = heard.trim().to_string();

    let preferred: String = dialoguer::Input::new()
        .with_prompt("Preferred (correct spelling)")
        .validate_with(|input: &String| {
            if input.trim().is_empty() {
                Err("cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    let preferred = preferred.trim().to_string();

    // Check for existing entry
    if let Some(existing) = dict.get(&heard) {
        eprintln!("  current: \"{heard}\" → \"{existing}\"");
        eprintln!("  new:     \"{heard}\" → \"{preferred}\"");

        let overwrite = dialoguer::Confirm::new()
            .with_prompt("Overwrite?")
            .default(true)
            .interact()?;

        if !overwrite {
            eprintln!("Cancelled.");
            return Ok(());
        }
    }

    dict.insert(&heard, &preferred);
    store.save(&dict)?;

    eprintln!("Saved: \"{heard}\" → \"{preferred}\"");
    Ok(())
}
