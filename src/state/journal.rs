//! Crash journal: what unsaved marks existed when the app stopped running.
//!
//! The file is rewritten whenever marks change and deleted on a clean exit, so
//! *finding* one at startup is itself the signal that the last session ended
//! unexpectedly. What happens then is the user's call —
//! [`Settings::recover_after_crash`](crate::state::settings::Settings):
//!
//! * off (default) — the journal is discarded and the PDFs on disk stay exactly
//!   as they were. An interrupted session never changes a file.
//! * on — the marks are restored into the reopened documents, still unsaved.
//!
//! Either way nothing is written into a PDF without an explicit save.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pdf::Mark;
use crate::state::storage;

pub const JOURNAL_FILE: &str = "session.journal.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub path: PathBuf,
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Journal {
    pub entries: Vec<JournalEntry>,
}

impl Journal {
    /// The journal left behind by the previous session, if there is one.
    pub fn take_previous() -> Option<Journal> {
        if !storage::exists(JOURNAL_FILE) {
            return None;
        }

        let journal: Journal = storage::load_or_default(JOURNAL_FILE);
        // Whatever happens next, this journal has been consumed.
        storage::remove(JOURNAL_FILE);

        // A file that was deleted while we weren't running can't be recovered
        // into, and its path may since have been reused.
        let entries: Vec<_> = journal
            .entries
            .into_iter()
            .filter(|entry| !entry.marks.is_empty() && entry.path.is_file())
            .collect();

        (!entries.is_empty()).then_some(Journal { entries })
    }

    /// Records the unsaved state of one document. Documents with no unsaved
    /// marks are dropped from the journal instead.
    pub fn record(&mut self, path: &Path, marks: &[Mark], dirty: bool) {
        self.entries.retain(|entry| entry.path != path);

        if dirty && !marks.is_empty() {
            self.entries.push(JournalEntry {
                path: path.to_path_buf(),
                marks: marks.to_vec(),
            });
        }

        self.flush();
    }

    pub fn forget(&mut self, path: &Path) {
        self.entries.retain(|entry| entry.path != path);
        self.flush();
    }

    /// Called on a clean exit: no unsaved work was lost, so there's nothing to
    /// recover next time.
    pub fn discard(&mut self) {
        self.entries.clear();
        storage::remove(JOURNAL_FILE);
    }

    fn flush(&self) {
        if self.entries.is_empty() {
            storage::remove(JOURNAL_FILE);
        } else {
            let _ = storage::save(JOURNAL_FILE, self);
        }
    }
}
