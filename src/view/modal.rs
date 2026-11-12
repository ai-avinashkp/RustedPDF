//! Modals, and what makes them modal.
//!
//! Two things stop the rest of the app responding while one is up:
//!
//! * a full-window backdrop with `occlude()`, which blocks every mouse event
//!   from reaching anything painted below it — the document, the toolbar, the
//!   tab strip and the titlebar included;
//! * a focus and key-context switch in [`crate::view`], so the workspace's key
//!   bindings stop resolving and only the modal's own keys (Escape, Enter) do.
//!
//! The backdrop is deferred above the menu layer, so an open dropdown can't
//! poke through either.

use gpui::{
    deferred, div, prelude::*, px, rgb, rgba, AnyElement, Context, MouseButton, MouseDownEvent,
    SharedString, Window,
};

use crate::app::{CloseChoice, CloseScope, Modal, Workspace, MODAL_LAYER};
use crate::integration;
use crate::pdf::mark::PRESET_COLORS;
use crate::pdf::ExportFormat;
use crate::state::settings::{MAX_ZOOM, MIN_ZOOM};
use crate::tab::ZOOM_STOPS;
use crate::theme::Palette;
use crate::view::widgets::{button, checkbox, divider, section_heading, swatch};

const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+O", "Open a PDF"),
    ("Ctrl+S", "Save the active document"),
    ("Ctrl+Shift+S", "Save a copy"),
    ("Ctrl+E", "Export pages as PNG or PDF"),
    ("Ctrl+W", "Close the tab"),
    ("Ctrl+Tab", "Next tab"),
    ("Ctrl+1 … Ctrl+4", "Select / Highlight / Underline / Erase"),
    ("Ctrl++ / Ctrl+−", "Zoom in and out (50%–200%)"),
    ("Ctrl+0", "Actual size"),
    ("Ctrl+G", "Go to a page number"),
    ("Ctrl+C", "Copy the selected text"),
    ("Ctrl+A", "Select all the text in the document"),
    ("Ctrl+wheel / pinch", "Zoom about the pointer"),
    (
        "Right-click",
        "Copy, mark or unmark the selection, switch tools",
    ),
    ("Ctrl+Z", "Remove the last mark"),
    ("Ctrl+,", "Settings"),
    ("Escape", "Close a menu or dialog"),
];

/// A dialog's contents: the part that scrolls, and the buttons that don't.
pub struct ModalContent {
    body: AnyElement,
    actions: Option<AnyElement>,
}

impl ModalContent {
    fn new(body: AnyElement) -> Self {
        Self {
            body,
            actions: None,
        }
    }

    /// Buttons are pinned below the scroll area — a dialog you can't reach the
    /// buttons of is a trap, however long its contents.
    fn with_actions(body: AnyElement, actions: AnyElement) -> Self {
        Self {
            body,
            actions: Some(actions),
        }
    }
}

impl Workspace {
    pub fn modal_layer(
        &self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(modal) = self.modal.as_ref() else {
            return div().into_any_element();
        };

        // A decision can't be dismissed by clicking away from it.
        let dismissible = !matches!(modal, Modal::ConfirmClose { .. });

        let (width, content) = match modal {
            Modal::Settings => (px(480.), self.settings_modal(palette, cx)),
            Modal::About => (px(420.), self.about_modal(palette)),
            Modal::Shortcuts => (px(460.), self.shortcuts_modal(palette)),
            Modal::Message { body, .. } => (px(460.), message_body(body.clone(), palette)),
            Modal::ConfirmClose { scope, targets } => {
                (px(480.), self.close_modal(*scope, targets, palette, cx))
            }
            Modal::Export { pages, format } => {
                (px(540.), self.export_modal(pages, *format, palette, cx))
            }
        };

        // Sized against the window, not a fixed number: a short window — or one
        // the user has just dragged smaller — must never push the title off the
        // top or the buttons off the bottom.
        let viewport = window.viewport_size();
        let max_height = (viewport.height - px(72.)).max(px(220.));
        let width = width.min(viewport.width - px(48.));

        let panel = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(width)
            .max_h(max_height)
            .p_4()
            .rounded_lg()
            .bg(rgb(palette.panel))
            .border_1()
            .border_color(rgb(palette.border))
            .shadow_lg()
            .text_color(rgb(palette.text))
            // Blocks the app behind the dialog from seeing the pointer at all.
            .occlude()
            // The backdrop below is also an ancestor, so a click inside the
            // dialog would bubble up to its dismiss handler without this.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .text_size(px(15.))
                    .text_color(rgb(palette.text))
                    .child(modal.title()),
            )
            .child(divider(palette))
            .child(
                div()
                    .id("modal-body")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    // Without this the body refuses to shrink and pushes the
                    // panel past its maximum instead of scrolling.
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(content.body),
            )
            .when_some(content.actions, |panel, actions| {
                panel
                    .child(divider(palette))
                    .child(div().flex_none().child(actions))
            });

        // One layer: the backdrop is also what centres the dialog. It has to be
        // a *direct* child of the root element — absolute positioning resolves
        // against the immediate parent, not the nearest positioned ancestor, so
        // wrapping this in anything else would size it to the wrapper.
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0000_0099))
                // Swallows every mouse event bound for the app behind it.
                .occlude()
                .when(dismissible, |backdrop| {
                    backdrop.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.dismiss_modal();
                            cx.notify();
                        }),
                    )
                })
                .child(panel),
        )
        .with_priority(MODAL_LAYER)
        .into_any_element()
    }

    // --- settings ---------------------------------------------------------

    fn settings_modal(&self, palette: Palette, cx: &mut Context<Self>) -> ModalContent {
        let settings = &self.settings;

        let mut zooms = div().flex().flex_row().items_center().gap(px(4.));
        for stop in ZOOM_STOPS {
            let stop = *stop;
            let selected = (settings.default_zoom - stop).abs() < 0.001;
            zooms = zooms.child(
                button(
                    format!("default-zoom-{}", (stop * 100.) as u32),
                    format!("{}%", (stop * 100.) as u32),
                    palette,
                    selected,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.settings.default_zoom = stop.clamp(MIN_ZOOM, MAX_ZOOM);
                        this.settings.save();
                        cx.notify();
                    }),
                ),
            );
        }

        let mut colors = div().flex().flex_row().items_center().gap(px(6.));
        for (name, preset) in PRESET_COLORS {
            let preset = *preset;
            colors = colors.child(
                swatch(
                    format!("settings-swatch-{name}"),
                    preset,
                    preset == settings.mark_color,
                    palette,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_mark_color(preset);
                        cx.notify();
                    }),
                ),
            );
        }

        let body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(toggle_row(
                "setting-recover",
                "Recover unsaved marks after a crash",
                "If the app is killed or crashes with unsaved marks, restore them on the next \
                 launch. When off, they're discarded and your PDFs are left untouched.",
                settings.recover_after_crash,
                palette,
                cx,
                |workspace| {
                    workspace.settings.recover_after_crash = !workspace.settings.recover_after_crash
                },
            ))
            .child(toggle_row(
                "setting-confirm",
                "Ask before closing with unsaved marks",
                "Shows the save dialog when closing a tab or the window.",
                settings.confirm_on_close,
                palette,
                cx,
                |workspace| {
                    workspace.settings.confirm_on_close = !workspace.settings.confirm_on_close
                },
            ))
            .child(toggle_row(
                "setting-theme",
                "Dark theme",
                "Also on Ctrl+Shift+T.",
                settings.dark_theme,
                palette,
                cx,
                |workspace| workspace.settings.dark_theme = !workspace.settings.dark_theme,
            ))
            .child(divider(palette))
            .child(section_heading("FILE MANAGER", palette))
            .child(self.open_with_row(palette, cx))
            .child(divider(palette))
            .child(section_heading("ZOOM FOR NEWLY OPENED DOCUMENTS", palette))
            .child(zooms)
            .child(section_heading("DEFAULT MARK COLOUR", palette))
            .child(colors)
            .child(divider(palette))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(palette.muted))
                            .child(format!(
                                "{} files in the recent list",
                                self.recents.files.len()
                            )),
                    )
                    .child(
                        button(
                            "settings-clear-recents",
                            "Clear recent files",
                            palette,
                            false,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.recents.clear();
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .into_any_element();

        let actions = div()
            .flex()
            .flex_row()
            .justify_end()
            .child(
                button("settings-done", "Done", palette, true).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.settings.save();
                        this.dismiss_modal();
                        cx.notify();
                    }),
                ),
            )
            .into_any_element();

        ModalContent::with_actions(body, actions)
    }

    /// Register / remove RustedPdf as a PDF handler, so right-clicking a PDF in
    /// the file manager offers it.
    fn open_with_row(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let registered = integration::is_registered();

        div()
            .flex()
            .flex_row()
            .items_start()
            .justify_between()
            .gap_3()
            .p_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .flex_1()
                    // A flex item won't shrink below its content unless told
                    // to; without this the description pushes the button out
                    // of the dialog.
                    .min_w(px(0.))
                    .child(if registered {
                        "Open PDFs with RustedPdf — registered"
                    } else {
                        "Open PDFs with RustedPdf"
                    })
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(palette.muted))
                            .child(integration::describe()),
                    ),
            )
            .child(
                button(
                    "open-with-toggle",
                    if registered { "Remove" } else { "Register" },
                    palette,
                    !registered,
                )
                .flex_none()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_open_with(!registered);
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    }

    // --- close confirmation ----------------------------------------------

    fn close_modal(
        &self,
        scope: CloseScope,
        targets: &[crate::app::CloseTarget],
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> ModalContent {
        let mut list = div().flex().flex_col().gap_1();

        for target in targets {
            let tab = target.tab;
            list = list.child(
                div()
                    .id(SharedString::from(format!("close-target-{tab}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py(px(4.))
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(palette.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.toggle_close_target(tab);
                            cx.notify();
                        }),
                    )
                    .child(checkbox(target.selected, palette))
                    .child(div().flex_1().child(target.name.clone()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(palette.muted))
                            .child(format!(
                                "{} mark{}",
                                target.marks,
                                if target.marks == 1 { "" } else { "s" }
                            )),
                    ),
            );
        }

        let body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_color(rgb(palette.muted)).child(
                "These documents have marks that aren't in the file yet. \
                     Tick the ones to write, or save them all.",
            ))
            .child(list)
            .into_any_element();

        let actions = div()
            .flex()
            .flex_row()
            .justify_between()
            .gap_2()
            .child(
                button("close-cancel", "Cancel", palette, false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_modal();
                        cx.notify();
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        button("close-discard", "Close without saving", palette, false)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    this.resolve_close(scope, CloseChoice::Discard, window, cx);
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        button("close-save-selected", "Save selected", palette, false)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    this.resolve_close(
                                        scope,
                                        CloseChoice::SaveSelected,
                                        window,
                                        cx,
                                    );
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        button("close-save-all", "Save all", palette, true).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                this.resolve_close(scope, CloseChoice::SaveAll, window, cx);
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .into_any_element();

        ModalContent::with_actions(body, actions)
    }

    // --- export -----------------------------------------------------------

    /// Pick pages and a format. Pages are chosen by clicking their numbers —
    /// a grid of every page, rather than a range you have to type correctly.
    fn export_modal(
        &self,
        pages: &[bool],
        format: ExportFormat,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> ModalContent {
        let chosen = pages.iter().filter(|selected| **selected).count();

        let mut grid = div().flex().flex_row().flex_wrap().gap(px(4.));
        for (index, selected) in pages.iter().enumerate() {
            let selected = *selected;
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("export-page-{index}")))
                    .min_w(px(30.))
                    .h(px(26.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(if selected {
                        palette.accent
                    } else {
                        palette.border
                    }))
                    .bg(rgb(if selected {
                        palette.accent
                    } else {
                        palette.panel
                    }))
                    .text_size(px(11.))
                    .text_color(rgb(if selected {
                        palette.accent_text
                    } else {
                        palette.text
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.set_export_page(index, !selected);
                            cx.notify();
                        }),
                    )
                    .child(format!("{}", index + 1)),
            );
        }

        let format_button = |value: ExportFormat, label: &'static str, cx: &mut Context<Self>| {
            button(
                SharedString::from(format!("export-format-{label}")),
                label,
                palette,
                value == format,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.set_export_format(value);
                    cx.notify();
                }),
            )
        };

        let body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section_heading("PAGES", palette))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        button("export-all", "Select all", palette, false).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.set_all_export_pages(true);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        button("export-none", "Clear", palette, false).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.set_all_export_pages(false);
                                cx.notify();
                            }),
                        ),
                    ),
            )
            // No scroll of its own: the dialog body already scrolls and the
            // buttons are pinned outside it, so a second scroll here would just
            // be a box inside a box.
            .child(grid)
            .child(divider(palette))
            .child(section_heading("FORMAT", palette))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(format_button(ExportFormat::Png, "PNG", cx))
                    .child(format_button(ExportFormat::Pdf, "PDF", cx)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(palette.muted))
                    .child(if chosen == 0 {
                        "No pages chosen.".to_string()
                    } else {
                        format!(
                            "{chosen} page{} → {}. Marks are included.",
                            if chosen == 1 { "" } else { "s" },
                            format.describe(chosen)
                        )
                    }),
            )
            .into_any_element();

        let actions = div()
            .flex()
            .flex_row()
            .justify_end()
            .gap_2()
            .child(
                button("export-cancel", "Cancel", palette, false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_modal();
                        cx.notify();
                    }),
                ),
            )
            .child(button("export-go", "Export…", palette, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.run_export();
                    cx.notify();
                }),
            ))
            .into_any_element();

        ModalContent::with_actions(body, actions)
    }

    // --- static content ---------------------------------------------------

    fn about_modal(&self, palette: Palette) -> ModalContent {
        ModalContent::new(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child("RustedPdf — a PDF reader and highlighter.")
                .child(
                    div()
                        .text_color(rgb(palette.muted))
                        .child("Built with Rust, GPUI and PDFium."),
                )
                .child(
                    div()
                        .text_color(rgb(palette.muted))
                        .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                )
                .into_any_element(),
        )
    }

    fn shortcuts_modal(&self, palette: Palette) -> ModalContent {
        let mut list = div().flex().flex_col().gap_1();

        for (keys, description) in SHORTCUTS {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .gap_4()
                    .child(div().text_color(rgb(palette.muted)).child(*keys))
                    .child(div().child(*description)),
            );
        }

        ModalContent::new(list.into_any_element())
    }
}

fn message_body(body: SharedString, palette: Palette) -> ModalContent {
    ModalContent::new(
        div()
            .text_color(rgb(palette.text))
            .child(body)
            .into_any_element(),
    )
}

/// A settings row: label, explanation, and a tick box. The whole row toggles.
fn toggle_row(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    checked: bool,
    palette: Palette,
    cx: &mut Context<Workspace>,
    apply: fn(&mut Workspace),
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_start()
        .gap_3()
        .p_2()
        .rounded_md()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(palette.hover)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |workspace: &mut Workspace, _: &MouseDownEvent, _, cx| {
                    apply(workspace);
                    workspace.settings.save();
                    cx.notify();
                },
            ),
        )
        .child(div().pt(px(2.)).child(checkbox(checked, palette)))
        .child(
            div().flex().flex_col().gap(px(2.)).child(label).child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(palette.muted))
                    .child(description),
            ),
        )
        .into_any_element()
}
