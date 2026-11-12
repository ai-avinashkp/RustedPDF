//! Colour palettes. Values are raw `0xRRGGBB` so this module stays free of any
//! UI-framework types; the view layer converts with `gpui::rgb(..)`.

use crate::ui::titlebar::TitleBarStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Main content background.
    pub background: u32,
    /// Titlebar / status strip.
    pub surface: u32,
    /// Floating menu panels.
    pub panel: u32,
    pub text: u32,
    pub muted: u32,
    pub disabled: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub border: u32,
    /// Hover fill for menu buttons and menu rows.
    pub hover: u32,
    /// Fill of an inactive tab in the tab strip.
    pub tab_inactive: u32,
    /// Fill for the menu button whose dropdown is open.
    pub active: u32,
    /// Close-button hover.
    pub danger: u32,
    /// Fill behind selected text — the same whether the drag is still going or
    /// has finished, because it is the same selection either way.
    pub selection: u32,
}

impl Palette {
    pub fn for_mode(mode: ThemeMode) -> Palette {
        match mode {
            ThemeMode::Light => Palette {
                background: 0xf7f7f8,
                surface: 0xececed,
                panel: 0xffffff,
                text: 0x1f1f1f,
                muted: 0x6b6b6b,
                disabled: 0xacacac,
                accent: 0xc0392b,
                accent_text: 0xffffff,
                border: 0xd4d4d6,
                hover: 0xdcdcde,
                tab_inactive: 0xe1e1e3,
                active: 0xcfcfd2,
                danger: 0xc42b1c,
                selection: 0x4a85ff,
            },
            ThemeMode::Dark => Palette {
                background: 0x1b1b1f,
                surface: 0x252529,
                panel: 0x2c2c31,
                text: 0xe6e6e8,
                muted: 0x9a9aa2,
                disabled: 0x5e5e66,
                accent: 0xe05545,
                accent_text: 0xffffff,
                border: 0x3a3a41,
                hover: 0x3a3a41,
                tab_inactive: 0x2c2c31,
                active: 0x44444c,
                danger: 0xc42b1c,
                selection: 0x4a85ff,
            },
        }
    }

    /// Maps this palette onto the reusable titlebar component's own style type.
    pub fn titlebar_style(&self) -> TitleBarStyle {
        TitleBarStyle {
            background: self.surface,
            border: self.border,
            panel: self.panel,
            text: self.text,
            muted: self.muted,
            disabled: self.disabled,
            hover: self.hover,
            active: self.active,
            mark_background: self.accent,
            mark_text: self.accent_text,
            close_hover: self.danger,
            close_hover_text: 0xffffff,
        }
    }
}
