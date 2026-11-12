//! The recently-opened list.
//!
//! Safety first: an entry is only ever a *reference* to a file, so a reference
//! that no longer resolves is worse than useless — it invites the app to act on
//! a path that now belongs to something else. Every load, every record and
//! every open prunes entries whose file is gone.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::state::storage;

pub const RECENTS_FILE: &str = "recent.json";
const MAX_ENTRIES: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    /// Unix seconds, for "opened 2 hours ago".
    pub opened_at: u64,
    pub page_count: usize,
}

impl RecentFile {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    /// The containing directory, for telling two files of the same name apart.
    pub fn directory(&self) -> String {
        self.path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn age(&self) -> String {
        let now = unix_seconds();
        let seconds = now.saturating_sub(self.opened_at);

        match seconds {
            0..=59 => "just now".to_string(),
            60..=3599 => format!("{} min ago", seconds / 60),
            3600..=86_399 => format!("{} h ago", seconds / 3600),
            _ => format!("{} d ago", seconds / 86_400),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Recents {
    pub files: Vec<RecentFile>,
}

impl Recents {
    /// Loads the list, dropping anything that no longer exists on disk.
    pub fn load() -> Self {
        let mut recents: Recents = storage::load_or_default(RECENTS_FILE);
        if recents.prune() > 0 {
            recents.save();
        }
        recents
    }

    pub fn save(&self) {
        let _ = storage::save(RECENTS_FILE, self);
    }

    /// Removes entries whose file has been deleted, moved or renamed. Returns
    /// how many went.
    pub fn prune(&mut self) -> usize {
        let before = self.files.len();
        self.files.retain(|entry| entry.path.is_file());
        before - self.files.len()
    }

    /// Moves `path` to the front, pruning dead entries at the same time.
    pub fn record(&mut self, path: &Path, page_count: usize) {
        self.files.retain(|entry| entry.path != path);
        self.files.insert(
            0,
            RecentFile {
                path: path.to_path_buf(),
                opened_at: unix_seconds(),
                page_count,
            },
        );

        self.prune();
        self.files.truncate(MAX_ENTRIES);
        self.save();
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|entry| entry.path != path);
        self.save();
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.save();
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
