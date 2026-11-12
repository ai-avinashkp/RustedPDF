//! JSON files in the user's config directory.
//!
//! Writes go to a temporary file and are then moved into place, so a crash
//! mid-write can't leave a half-written settings or journal file behind — which
//! matters most for the journal, whose whole job is to survive a crash.

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use serde::{de::DeserializeOwned, Serialize};

/// `%APPDATA%\RustedPdf`, `~/Library/Application Support/RustedPdf`, or
/// `~/.config/rustedpdf`.
pub fn config_dir() -> Option<PathBuf> {
    let directories = ProjectDirs::from("", "", "RustedPdf")?;
    let directory = directories.config_dir().to_path_buf();
    fs::create_dir_all(&directory).ok()?;
    Some(directory)
}

pub fn path_for(file: &str) -> Option<PathBuf> {
    Some(config_dir()?.join(file))
}

pub fn exists(file: &str) -> bool {
    path_for(file).is_some_and(|path| path.exists())
}

/// Reads `file`, falling back to `Default` when it's missing or unreadable —
/// a corrupt settings file shouldn't stop the app from starting.
pub fn load_or_default<T: DeserializeOwned + Default>(file: &str) -> T {
    let Some(path) = path_for(file) else {
        return T::default();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return T::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

pub fn save<T: Serialize>(file: &str, value: &T) -> Result<()> {
    let path = path_for(file).ok_or_else(|| anyhow!("No config directory available"))?;
    let temporary = path.with_extension("tmp");

    fs::write(&temporary, serde_json::to_string_pretty(value)?)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

pub fn remove(file: &str) {
    if let Some(path) = path_for(file) {
        let _ = fs::remove_file(path);
    }
}
