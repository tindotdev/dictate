//! Private helpers for config-backed JSON persistence.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Serialize, de::DeserializeOwned};

pub fn open_config_path(filename: &str) -> Option<PathBuf> {
    let dirs = ProjectDirs::from("dev", "tin", "dictate")?;
    Some(dirs.config_dir().join(filename))
}

pub fn load_json_file<T, E>(path: &Path) -> Result<T, E>
where
    T: DeserializeOwned + Default,
    E: From<std::io::Error> + From<serde_json::Error>,
{
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(err.into()),
    }
}

pub fn save_json_file<T, E>(path: &Path, tmp_filename: &str, value: &T) -> Result<(), E>
where
    T: Serialize,
    E: From<std::io::Error> + From<serde_json::Error>,
{
    let json = serde_json::to_string_pretty(value)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_file_name(tmp_filename);
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, path)?;

    Ok(())
}
