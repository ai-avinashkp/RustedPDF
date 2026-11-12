//! Small shared pieces of chrome. Callers attach their own click handlers.

use gpui::{div, prelude::*, px, rgb, Div, SharedString, Stateful};

use crate::theme::Palette;

/// A labelled button. `primary` marks the default action in a dialog.
pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    palette: Palette,
    primary: bool,
) -> Stateful<Div> {
    let (background, foreground, border) = if primary {
        (palette.accent, palette.accent_text, palette.accent)
    } else {
        (palette.surface, palette.text, palette.border)
    };

    div()
        .id(id.into())
        .px_3()
        .py(px(5.))
        .rounded_md()
        .cursor_pointer()
        .whitespace_nowrap()
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .text_color(rgb(foreground))
        .hover(move |style| {
            style.bg(rgb(if primary {
                palette.accent
            } else {
                palette.hover
            }))
        })
        .child(label.into())
}

/// A square button holding one glyph — the toolbar's tools and zoom controls.
pub fn icon_button(
    id: impl Into<SharedString>,
    glyph: impl Into<SharedString>,
    palette: Palette,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .w(px(28.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(if active {
            palette.accent_text
        } else {
            palette.text
        }))
        .when(active, |element| element.bg(rgb(palette.accent)))
        .hover(move |style| {
            style.bg(rgb(if active {
                palette.accent
            } else {
                palette.hover
            }))
        })
        .child(glyph.into())
}

/// A tick box. Clicking is wired up by the caller, usually on the whole row.
pub fn checkbox(checked: bool, palette: Palette) -> Div {
    div()
        .w(px(16.))
        .h(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(rgb(if checked {
            palette.accent
        } else {
            palette.border
        }))
        .bg(rgb(if checked {
            palette.accent
        } else {
            palette.panel
        }))
        .text_size(px(11.))
        .text_color(rgb(palette.accent_text))
        .child(if checked { "✓" } else { "" })
}

/// A colour swatch, ringed when it's the current choice.
pub fn swatch(
    id: impl Into<SharedString>,
    color: u32,
    selected: bool,
    palette: Palette,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .w(px(22.))
        .h(px(22.))
        .rounded_md()
        .cursor_pointer()
        .border_2()
        .border_color(rgb(if selected {
            palette.text
        } else {
            palette.border
        }))
        .bg(rgb(color))
}

pub fn section_heading(label: impl Into<SharedString>, palette: Palette) -> Div {
    div()
        .text_size(px(11.))
        .text_color(rgb(palette.muted))
        .child(label.into())
}

/// A horizontal rule.
pub fn divider(palette: Palette) -> Div {
    div().h(px(1.)).w_full().bg(rgb(palette.border))
}

/// A vertical rule, for separating toolbar groups.
pub fn spacer(palette: Palette) -> Div {
    div().w(px(1.)).h(px(18.)).mx_1().bg(rgb(palette.border))
}
