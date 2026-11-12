//! Everything that outlives a session: settings, the recent list, and the
//! crash journal.

pub mod journal;
pub mod recents;
pub mod settings;
pub mod storage;

pub use journal::Journal;
pub use recents::Recents;
pub use settings::Settings;
