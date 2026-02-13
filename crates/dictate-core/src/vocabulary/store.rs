//! Persistent storage for the vocabulary.
//!
//! Reads and writes `vocabulary.json` in the platform-standard config directory.
//! Uses atomic write (write to `.tmp`, then rename) to prevent corruption.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::{Vocabulary, VocabularyError};

/// File name for the vocabulary store.
const VOCABULARY_FILENAME: &str = "vocabulary.json";

/// Temporary file used during atomic writes (co-located for same-fs rename).
const VOCABULARY_TMP_FILENAME: &str = "vocabulary.json.tmp";

/// Persistent storage for the vocabulary.
pub struct VocabularyStore {
    path: PathBuf,
}

impl VocabularyStore {
    /// Create a store using the platform-standard config directory.
    ///
    /// Does not touch the filesystem — the directory is created on first write.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError::NoConfigDir`] if the platform config directory
    /// cannot be determined.
    pub fn open() -> Result<Self, VocabularyError> {
        let dirs =
            ProjectDirs::from("dev", "tin", "dictate").ok_or(VocabularyError::NoConfigDir)?;
        let path = dirs.config_dir().join(VOCABULARY_FILENAME);
        Ok(Self { path })
    }

    /// Create a store at a specific path (for testing).
    #[must_use]
    pub const fn open_at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load the vocabulary from disk.
    ///
    /// Returns an empty vocabulary if the file does not exist.
    /// Returns an error if the file exists but contains invalid JSON.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError::Io`] on read failure or
    /// [`VocabularyError::InvalidJson`] if the file contains malformed JSON.
    pub fn load(&self) -> Result<Vocabulary, VocabularyError> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => {
                let vocab: Vocabulary = serde_json::from_str(&contents)?;
                Ok(vocab)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vocabulary::new()),
            Err(err) => Err(VocabularyError::Io(err)),
        }
    }

    /// Save the vocabulary to disk (atomic write).
    ///
    /// Strategy: serialize → write to `.tmp` → rename over target.
    /// Parent directories are created on first write.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError::Io`] on write or rename failure, or
    /// [`VocabularyError::InvalidJson`] on serialization failure.
    pub fn save(&self, vocab: &Vocabulary) -> Result<(), VocabularyError> {
        let json = serde_json::to_string_pretty(vocab)?;

        // Ensure parent directory exists.
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Atomic write: write to temp, then rename.
        let tmp_path = self.path.with_file_name(VOCABULARY_TMP_FILENAME);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &self.path)?;

        Ok(())
    }

    /// The resolved file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a store in a temporary directory.
    fn temp_store() -> (tempfile::TempDir, VocabularyStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(VOCABULARY_FILENAME);
        let store = VocabularyStore::open_at(path);
        (dir, store)
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let (_dir, store) = temp_store();
        let vocab = store.load().unwrap();
        assert!(vocab.is_empty());
    }

    #[test]
    fn load_valid_json() {
        let (_dir, store) = temp_store();
        std::fs::write(store.path(), r#"["AWS", "OpenAI"]"#).unwrap();

        let vocab = store.load().unwrap();
        assert!(vocab.contains("AWS"));
        assert!(vocab.contains("OpenAI"));
        assert_eq!(vocab.len(), 2);
    }

    #[test]
    fn load_malformed_json_returns_error() {
        let (_dir, store) = temp_store();
        std::fs::write(store.path(), "not json at all").unwrap();

        let result = store.load();
        assert!(matches!(result, Err(VocabularyError::InvalidJson(_))));
    }

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, store) = temp_store();

        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        vocab.insert("OpenAI");
        store.save(&vocab).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(vocab, loaded);
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("vocab.json");
        let store = VocabularyStore::open_at(nested.clone());

        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        store.save(&vocab).unwrap();

        assert!(nested.exists());
    }

    #[test]
    fn save_cleans_up_tmp_file() {
        let (_dir, store) = temp_store();

        let mut vocab = Vocabulary::new();
        vocab.insert("AWS");
        store.save(&vocab).unwrap();

        let tmp_path = store.path().with_file_name(VOCABULARY_TMP_FILENAME);
        assert!(
            !tmp_path.exists(),
            "temp file should be removed after rename"
        );
    }

    #[test]
    fn path_returns_correct_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        let store = VocabularyStore::open_at(path.clone());
        assert_eq!(store.path(), path);
    }

    #[test]
    fn open_resolves_platform_path() {
        // Accept NoConfigDir as valid (CI / headless environments may lack a home dir).
        match VocabularyStore::open() {
            Ok(store) => assert!(store.path().ends_with(VOCABULARY_FILENAME)),
            Err(VocabularyError::NoConfigDir) => {}
            Err(err) => panic!("unexpected error: {err}"),
        }
    }
}
