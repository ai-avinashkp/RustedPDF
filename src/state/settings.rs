//! User settings, persisted as JSON.

use serde::{Deserialize, Serialize};

use crate::state::storage;

pub const SETTINGS_FILE: &str = "settings.json";

/// Zoom is clamped to this range everywhere in the app.
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// What happens to unsaved marks after a crash or a forced quit.
    ///
    /// * `true` — the next launch restores them from the journal.
    /// * `false` — they're discarded, and the PDF on disk is left exactly as it
    ///   was. This is the default: an interrupted session shouldn't silently
    ///   change a file.
    pub recover_after_crash: bool,
    pub dark_theme: bool,
    /// `0xRRGGBB` used for new marks.
    pub mark_color: u32,
    /// How strongly new marks are applied, 0.2–1.0.
    pub mark_opacity: f32,
    /// Zoom applied to newly opened documents.
    pub default_zoom: f32,
    /// Ask before closing with unsaved marks. Turning this off closes without
    /// saving, so it stays on unless deliberately changed.
    pub confirm_on_close: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recover_after_crash: false,
            dark_theme: false,
            mark_color: 0xffd400,
            mark_opacity: 1.0,
            default_zoom: 1.0,
            confirm_on_close: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut settings: Settings = storage::load_or_default(SETTINGS_FILE);
        settings.default_zoom = settings.default_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        settings.mark_opacity = settings.mark_opacity.clamp(0.05, 1.0);
        settings
    }

    pub fn save(&self) {
        let _ = storage::save(SETTINGS_FILE, self);
    }
}
