//! Colours and metrics for [`super::TitleBar`].
//!
//! Colours are raw `0xRRGGBB`, so a host app can map its own palette onto this
//! without the component depending on the app's theme types.

use gpui::{px, Pixels};

#[derive(Debug, Clone, Copy)]
pub struct TitleBarStyle {
    /// Fill of the bar itself.
    pub background: u32,
    /// Bottom border of the bar, and the border of menu panels.
    pub border: u32,
    /// Fill of floating menu panels.
    pub panel: u32,
    pub text: u32,
    /// Secondary text: the window title, shortcuts, carets.
    pub muted: u32,
    /// Text of a command that can't be invoked.
    pub disabled: u32,
    /// Hover fill for menu buttons, menu rows and window buttons.
    pub hover: u32,
    /// Fill of the menu button whose dropdown is open.
    pub active: u32,
    pub mark_background: u32,
    pub mark_text: u32,
    pub close_hover: u32,
    pub close_hover_text: u32,
}

impl Default for TitleBarStyle {
    fn default() -> Self {
        Self {
            background: 0xececed,
            border: 0xd4d4d6,
            panel: 0xffffff,
            text: 0x1f1f1f,
            muted: 0x6b6b6b,
            disabled: 0xacacac,
            hover: 0xdcdcde,
            active: 0xcfcfd2,
            mark_background: 0x2563eb,
            mark_text: 0xffffff,
            close_hover: 0xc42b1c,
            close_hover_text: 0xffffff,
        }
    }
}

/// Sizes used by the bar. The defaults match a Windows 11-ish titlebar; the
/// dropdown geometry is derived from these, so overriding one value keeps the
/// menus aligned.
#[derive(Debug, Clone, Copy)]
pub struct TitleBarMetrics {
    /// Height of the strip.
    pub height: Pixels,
    /// Height of a top-level menu button.
    pub nav_button_height: Pixels,
    /// Width of every menu panel; submenus are positioned off this.
    pub panel_width: Pixels,
    /// Vertical padding inside a menu panel.
    pub panel_padding: Pixels,
    /// Height of one menu row.
    pub row_height: Pixels,
    /// Height of a separator row.
    pub separator_height: Pixels,
    /// How far a submenu overlaps its parent panel, so diagonal pointer travel
    /// never crosses a gap and closes the menu.
    pub submenu_overlap: Pixels,
    /// Width of one window-control button.
    pub control_width: Pixels,
    /// Space reserved on the left for the macOS traffic lights.
    pub traffic_lights_width: Pixels,
    /// Pointer travel before a press on the bar becomes a window drag.
    pub drag_threshold: Pixels,
}

impl Default for TitleBarMetrics {
    fn default() -> Self {
        Self {
            height: px(36.),
            nav_button_height: px(26.),
            panel_width: px(248.),
            panel_padding: px(5.),
            row_height: px(26.),
            separator_height: px(9.),
            submenu_overlap: px(4.),
            control_width: px(44.),
            traffic_lights_width: px(72.),
            drag_threshold: px(4.),
        }
    }
}

impl TitleBarMetrics {
    /// Offset of a dropdown from the top of its menu button, so the panel
    /// starts exactly at the bottom edge of the bar.
    ///
    /// The button is centred vertically, so the gap below it is half the
    /// leftover height.
    pub fn dropdown_top(&self) -> Pixels {
        self.nav_button_height + (self.height - self.nav_button_height) / 2.
    }
}
