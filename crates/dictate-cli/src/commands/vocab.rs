//! `dictate vocab` command family.

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, ExitStatus};

use dictate_core::{Vocabulary, VocabularyError, VocabularyStore};
use shlex::split as split_shell_words;
use tempfile::{Builder, NamedTempFile};
use thiserror::Error;

use crate::args::{VocabArgs, VocabCommand};

const EDIT_TEMPLATE_HEADER_LINES: [&str; 3] = [
    "# Edit vocabulary words below, one word per line.",
    "# The initial comment block is ignored when parsing this file.",
    "# Save and close the editor to apply changes.",
];

/// Errors from the `vocab` command.
#[derive(Debug, Error)]
pub enum VocabError {
    /// Vocabulary store error (load/save/path resolution).
    #[error(transparent)]
    Vocabulary(#[from] VocabularyError),

    /// IO error while preparing or reading the editor file.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// No editor command is configured.
    #[error("no editor configured; set $VISUAL or $EDITOR")]
    MissingEditor,

    /// Editor command cannot be parsed.
    #[error("failed to parse editor command `{0}` from $VISUAL/$EDITOR")]
    InvalidEditorCommand(String),

    /// Editor exited with non-zero status.
    #[error("editor exited unsuccessfully: {0}")]
    EditorFailed(ExitStatus),
}

/// Run the `vocab` command family.
pub fn run(args: &VocabArgs) -> Result<(), VocabError> {
    match &args.command {
        VocabCommand::Add { words } => run_add(words),
        VocabCommand::Remove { words } => run_remove(words),
        VocabCommand::List => run_list(),
        VocabCommand::Edit => run_edit(),
    }
}

fn run_add(words: &[String]) -> Result<(), VocabError> {
    let vocab_store = VocabularyStore::open()?;
    let mut vocab = vocab_store.load()?;

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

    Ok(())
}

fn run_edit() -> Result<(), VocabError> {
    let vocab_store = VocabularyStore::open()?;
    let current_vocab = vocab_store.load()?;
    let editor = resolve_editor()?.ok_or(VocabError::MissingEditor)?;
    let mut temp_file = Builder::new()
        .prefix("dictate-vocab-")
        .suffix(".txt")
        .tempfile()?;

    write_editor_template(&mut temp_file, &current_vocab)?;

    let status = Command::new(&editor.program)
        .args(&editor.args)
        .arg(temp_file.path())
        .status()?;
    if !status.success() {
        return Err(VocabError::EditorFailed(status));
    }

    let contents = std::fs::read_to_string(temp_file.path())?;
    let edited_vocab = parse_edited_vocabulary(&contents);

    if edited_vocab == current_vocab {
        eprintln!("Vocabulary unchanged: {} words", current_vocab.len());
        return Ok(());
    }

    let (added, removed) = diff_counts(&current_vocab, &edited_vocab);
    vocab_store.save(&edited_vocab)?;

    eprintln!(
        "Vocabulary updated: {} words (+{}, -{})",
        edited_vocab.len(),
        added,
        removed
    );

    Ok(())
}

fn write_editor_template(
    temp_file: &mut NamedTempFile,
    vocab: &Vocabulary,
) -> Result<(), VocabError> {
    let mut content = EDIT_TEMPLATE_HEADER_LINES.join("\n");
    content.push_str("\n\n");

    for word in vocab.iter() {
        content.push_str(word);
        content.push('\n');
    }

    let file = temp_file.as_file_mut();
    file.write_all(content.as_bytes())?;
    file.flush()?;
    Ok(())
}

fn parse_edited_vocabulary(contents: &str) -> Vocabulary {
    let mut vocab = Vocabulary::new();
    let trimmed_lines: Vec<&str> = contents.lines().map(str::trim).collect();
    let start_index = template_preamble_end_index(&trimmed_lines).unwrap_or(0);

    for trimmed in trimmed_lines.iter().skip(start_index) {
        if trimmed.is_empty() {
            continue;
        }

        let _ = vocab.insert(*trimmed);
    }

    vocab
}

fn template_preamble_end_index(lines: &[&str]) -> Option<usize> {
    if lines.len() < EDIT_TEMPLATE_HEADER_LINES.len() {
        return None;
    }

    if !lines
        .iter()
        .take(EDIT_TEMPLATE_HEADER_LINES.len())
        .eq(EDIT_TEMPLATE_HEADER_LINES.iter())
    {
        return None;
    }

    if lines
        .get(EDIT_TEMPLATE_HEADER_LINES.len())
        .is_some_and(|line| line.is_empty())
    {
        return Some(EDIT_TEMPLATE_HEADER_LINES.len() + 1);
    }

    Some(EDIT_TEMPLATE_HEADER_LINES.len())
}

#[derive(Debug, PartialEq, Eq)]
struct EditorCommand {
    program: String,
    args: Vec<String>,
}

fn resolve_editor() -> Result<Option<EditorCommand>, VocabError> {
    let visual = std::env::var("VISUAL").ok();
    let editor = std::env::var("EDITOR").ok();
    resolve_editor_from_env(visual.as_deref(), editor.as_deref())
}

fn resolve_editor_from_env(
    visual: Option<&str>,
    editor: Option<&str>,
) -> Result<Option<EditorCommand>, VocabError> {
    for value in [visual, editor].into_iter().flatten() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        return Ok(Some(parse_editor_command(trimmed)?));
    }

    Ok(None)
}

fn parse_editor_command(value: &str) -> Result<EditorCommand, VocabError> {
    let mut tokens = split_shell_words(value)
        .ok_or_else(|| VocabError::InvalidEditorCommand(value.to_string()))?
        .into_iter();

    let program = tokens
        .next()
        .ok_or_else(|| VocabError::InvalidEditorCommand(value.to_string()))?;
    let args = tokens.collect();

    Ok(EditorCommand { program, args })
}

fn diff_counts(before: &Vocabulary, after: &Vocabulary) -> (usize, usize) {
    let before_set: BTreeSet<String> = before.iter().map(str::to_string).collect();
    let after_set: BTreeSet<String> = after.iter().map(str::to_string).collect();

    let added = after_set.difference(&before_set).count();
    let removed = before_set.difference(&after_set).count();
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_edited_vocabulary_ignores_template_comments_and_blank_lines() {
        let contents = "# Edit vocabulary words below, one word per line.\n\
# The initial comment block is ignored when parsing this file.\n\
# Save and close the editor to apply changes.\n\
\n\
AWS\n\
  \n\
OpenAI\n";

        let vocab = parse_edited_vocabulary(contents);

        assert_eq!(vocab.iter().collect::<Vec<_>>(), vec!["AWS", "OpenAI"]);
    }

    #[test]
    fn parse_edited_vocabulary_deduplicates_words() {
        let contents = "AWS\nAWS\nOpenAI\n";

        let vocab = parse_edited_vocabulary(contents);

        assert_eq!(vocab.iter().collect::<Vec<_>>(), vec!["AWS", "OpenAI"]);
    }

    #[test]
    fn parse_edited_vocabulary_preserves_hash_prefixed_words() {
        let contents = "# Edit vocabulary words below, one word per line.\n\
# The initial comment block is ignored when parsing this file.\n\
# Save and close the editor to apply changes.\n\
\n\
#\n\
# topic\n\
#hashtag\n\
#define\n\
OpenAI\n";

        let vocab = parse_edited_vocabulary(contents);

        assert_eq!(
            vocab.iter().collect::<Vec<_>>(),
            vec!["#", "# topic", "#define", "#hashtag", "OpenAI"]
        );
    }

    #[test]
    fn parse_edited_vocabulary_ignores_template_comments_without_separator_line() {
        let contents = "# Edit vocabulary words below, one word per line.\n\
# The initial comment block is ignored when parsing this file.\n\
# Save and close the editor to apply changes.\n\
OpenAI\n\
Rust\n";

        let vocab = parse_edited_vocabulary(contents);

        assert_eq!(vocab.iter().collect::<Vec<_>>(), vec!["OpenAI", "Rust"]);
    }

    #[test]
    fn parse_edited_vocabulary_handles_empty_template_without_separator_line() {
        let contents = "# Edit vocabulary words below, one word per line.\n\
# The initial comment block is ignored when parsing this file.\n\
# Save and close the editor to apply changes.";

        let vocab = parse_edited_vocabulary(contents);

        assert!(vocab.is_empty());
    }

    #[test]
    fn parse_edited_vocabulary_round_trips_empty_template_with_trimmed_trailing_blank_lines() {
        let mut temp_file = Builder::new()
            .prefix("dictate-vocab-test-")
            .suffix(".txt")
            .tempfile()
            .unwrap();
        let vocab = Vocabulary::new();

        write_editor_template(&mut temp_file, &vocab).unwrap();

        let contents = std::fs::read_to_string(temp_file.path()).unwrap();
        let trimmed = contents.trim_end_matches(char::is_whitespace);
        let edited_vocab = parse_edited_vocabulary(trimmed);

        assert!(edited_vocab.is_empty());
    }

    #[test]
    fn parse_edited_vocabulary_keeps_hash_space_line_without_template() {
        let contents = "# topic\n#\nOpenAI\n";

        let vocab = parse_edited_vocabulary(contents);

        assert_eq!(
            vocab.iter().collect::<Vec<_>>(),
            vec!["#", "# topic", "OpenAI"]
        );
    }

    #[test]
    fn resolve_editor_prefers_visual() {
        let editor = resolve_editor_from_env(Some("nvim"), Some("vim")).unwrap();
        assert_eq!(
            editor,
            Some(EditorCommand {
                program: "nvim".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn resolve_editor_falls_back_to_editor() {
        let editor = resolve_editor_from_env(None, Some("vim")).unwrap();
        assert_eq!(
            editor,
            Some(EditorCommand {
                program: "vim".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn resolve_editor_ignores_empty_values() {
        let editor = resolve_editor_from_env(Some("   "), Some("  emacs  ")).unwrap();
        assert_eq!(
            editor,
            Some(EditorCommand {
                program: "emacs".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn resolve_editor_parses_args_from_visual() {
        let editor = resolve_editor_from_env(Some("code --wait"), Some("vim")).unwrap();
        assert_eq!(
            editor,
            Some(EditorCommand {
                program: "code".to_string(),
                args: vec!["--wait".to_string()],
            })
        );
    }

    #[test]
    fn resolve_editor_preserves_quoted_arguments() {
        let editor = resolve_editor_from_env(Some("nvim --cmd \"set title\""), None).unwrap();
        assert_eq!(
            editor,
            Some(EditorCommand {
                program: "nvim".to_string(),
                args: vec!["--cmd".to_string(), "set title".to_string()],
            })
        );
    }

    #[test]
    fn resolve_editor_reports_invalid_shell_syntax() {
        let error = resolve_editor_from_env(Some("code --wait \"unterminated"), None).unwrap_err();
        assert!(matches!(error, VocabError::InvalidEditorCommand(_)));
    }

    #[test]
    fn diff_counts_reports_added_and_removed() {
        let mut before = Vocabulary::new();
        let _ = before.insert("AWS");
        let _ = before.insert("OpenAI");

        let mut after = Vocabulary::new();
        let _ = after.insert("OpenAI");
        let _ = after.insert("Kubernetes");

        assert_eq!(diff_counts(&before, &after), (1, 1));
    }
}
