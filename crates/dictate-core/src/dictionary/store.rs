//! Persistent storage for the dictionary.
//!
//! Reads and writes `dictionary.json` in the platform-standard config directory.
//! Uses atomic write (write to `.tmp`, then rename) to prevent corruption.

use std::path::{Path, PathBuf};

use super::{Dictionary, DictionaryError};
use crate::config_store;

/// File name for the dictionary store.
const DICTIONARY_FILENAME: &str = "dictionary.json";

/// Temporary file used during atomic writes (co-located for same-fs rename).
const DICTIONARY_TMP_FILENAME: &str = "dictionary.json.tmp";

/// Persistent storage for the dictionary.
pub struct DictionaryStore {
    path: PathBuf,
}

impl DictionaryStore {
    /// Create a store using the platform-standard config directory.
    ///
    /// Does not touch the filesystem — the directory is created on first write.
    ///
    /// # Errors
    ///
    /// Returns [`DictionaryError::NoConfigDir`] if the platform config directory
    /// cannot be determined.
    pub fn open() -> Result<Self, DictionaryError> {
        let path = config_store::open_config_path(DICTIONARY_FILENAME)
            .ok_or(DictionaryError::NoConfigDir)?;
        Ok(Self { path })
    }

    /// Create a store at a specific path (for testing).
    #[must_use]
    pub const fn open_at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load the dictionary from disk.
    ///
    /// Returns an empty dictionary if the file does not exist.
    /// Returns an error if the file exists but contains invalid JSON.
    ///
    /// # Errors
    ///
    /// Returns [`DictionaryError::Io`] on read failure or
    /// [`DictionaryError::InvalidJson`] if the file contains malformed JSON.
    pub fn load(&self) -> Result<Dictionary, DictionaryError> {
        config_store::load_json_file(&self.path)
    }

    /// Save the dictionary to disk (atomic write).
    ///
    /// Strategy: serialize → write to `.tmp` → rename over target.
    /// Parent directories are created on first write.
    ///
    /// # Errors
    ///
    /// Returns [`DictionaryError::Io`] on write or rename failure, or
    /// [`DictionaryError::InvalidJson`] on serialization failure.
    pub fn save(&self, dict: &Dictionary) -> Result<(), DictionaryError> {
        config_store::save_json_file(&self.path, DICTIONARY_TMP_FILENAME, dict)
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
    fn temp_store() -> (tempfile::TempDir, DictionaryStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DICTIONARY_FILENAME);
        let store = DictionaryStore::open_at(path);
        (dir, store)
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let (_dir, store) = temp_store();
        let dict = store.load().unwrap();
        assert!(dict.is_empty());
    }

    #[test]
    fn load_valid_json() {
        let (_dir, store) = temp_store();
        std::fs::write(store.path(), r#"{"Cloud": "Claude"}"#).unwrap();

        let dict = store.load().unwrap();
        assert_eq!(dict.get("Cloud"), Some("Claude"));
    }

    #[test]
    fn load_malformed_json_returns_error() {
        let (_dir, store) = temp_store();
        std::fs::write(store.path(), "not json at all").unwrap();

        let result = store.load();
        assert!(matches!(result, Err(DictionaryError::InvalidJson(_))));
    }

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, store) = temp_store();

        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        dict.insert("Tim", "Tin");
        store.save(&dict).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(dict, loaded);
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("dict.json");
        let store = DictionaryStore::open_at(nested.clone());

        let mut dict = Dictionary::new();
        dict.insert("test", "value");
        store.save(&dict).unwrap();

        assert!(nested.exists());
    }

    #[test]
    fn save_cleans_up_tmp_file() {
        let (_dir, store) = temp_store();

        let mut dict = Dictionary::new();
        dict.insert("Cloud", "Claude");
        store.save(&dict).unwrap();

        let tmp_path = store.path().with_file_name(DICTIONARY_TMP_FILENAME);
        assert!(
            !tmp_path.exists(),
            "temp file should be removed after rename"
        );
    }

    #[test]
    fn path_returns_correct_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        let store = DictionaryStore::open_at(path.clone());
        assert_eq!(store.path(), path);
    }

    #[test]
    fn open_resolves_platform_path() {
        // Accept NoConfigDir as valid (CI / headless environments may lack a home dir).
        match DictionaryStore::open() {
            Ok(store) => assert!(store.path().ends_with(DICTIONARY_FILENAME)),
            Err(DictionaryError::NoConfigDir) => {}
            Err(err) => panic!("unexpected error: {err}"),
        }
    }
}
