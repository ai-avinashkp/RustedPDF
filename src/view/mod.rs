//! Rendering. Each submodule owns one region of the window and hangs its
//! methods off [`Workspace`], so the composition below reads as the layout.

pub mod context_menu;
pub mod modal;
pub mod scrollbar;
pub mod start;
pub mod tabs;
pub mod toolbar;
pub mod viewer;
pub mod widgets;

use gpui::{
    deferred, div, prelude::*, px, rgb, Context, Div, FocusHandle, MouseButton, MouseDownEvent,
    Window,
};

use crate::app::{
    CloseTab, CopySelection, Dismiss, ExportPages, GoToPage, NextTab, OpenFile, OpenSettings,
    PrevTab, SaveAll, SaveFile, SaveFileAs, SelectAll, ToggleTheme, ToolErase, ToolHighlight,
    ToolSelect, ToolUnderline, UndoMark, Workspace, ZoomIn, ZoomOut, ZoomReset, BACKDROP_LAYER,
    UI_FONT,
};
use crate::menu::MENUS;
use crate::theme::Palette;
use crate::ui::titlebar::TitleBar;

impl Workspace {
    fn status_bar(&self, palette: Palette, cx: &mut Context<Self>) -> Div {
        // The file name is a link to the file: clicking it shows it in
        // Explorer (or Finder), selected.
        let document = self.active_tab().map(|tab| {
            let marks = tab.document.marks.len();
            div()
                .flex()
                .flex_row()
                .child(
                    div()
                        .id("status-file")
                        .cursor_pointer()
                        .hover(move |style| style.text_color(rgb(palette.text)).underline())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                this.run_command("file.reveal", window, cx);
                            }),
                        )
                        .child(tab.title()),
                )
                .child(format!(
                    "{} · {} mark{}",
                    if tab.is_dirty() { " (unsaved)" } else { "" },
                    marks,
                    if marks == 1 { "" } else { "s" }
                ))
        });

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .w_full()
            .h(px(24.))
            .px_3()
            .bg(rgb(palette.surface))
            .border_t_1()
            .border_color(rgb(palette.border))
            .text_size(px(12.))
            .text_color(rgb(palette.muted))
            // The bar is one line tall and both halves can be long — a status
            // naming a file, a summary naming the same file. Left to wrap,
            // whichever half loses the fight for width folds into a tall
            // column that spills up over the document. Neither wraps; the
            // status is the one that gives way, because the counters on the
            // right are short and fixed and it is the sentence.
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .when_some(document, |bar, summary| bar.child(summary))
                    .child(self.tool.label())
                    .child(format!("{}%", self.zoom_percent())),
            )
    }

    /// Invisible layer that closes an open menu on the next click anywhere.
    fn menu_backdrop(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div()
                .absolute()
                .inset_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.title_bar.menu.close();
                        this.color_dropdown_open = false;
                        this.close_page_input();
                        this.close_context_menu();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        let had_context_menu = this.close_context_menu();
                        this.title_bar.menu.close();
                        this.color_dropdown_open = false;
                        this.close_page_input();

                        // Right-clicking elsewhere in the document with the
                        // context menu up moves it there, as everywhere else.
                        let over_document = this
                            .active_tab()
                            .is_some_and(|tab| tab.scroll.bounds().contains(&event.position));
                        if had_context_menu && over_document {
                            this.open_context_menu(event.position);
                        }
                        cx.notify();
                    }),
                ),
        )
        .with_priority(BACKDROP_LAYER)
    }
}

/// Binds the app's actions. Keeping them together means a key binding and the
/// matching menu item can't drift apart — both end in `run_command`.
fn with_actions(root: Div, cx: &mut Context<Workspace>) -> Div {
    /// `action => command id`.
    macro_rules! command {
        ($root:expr, $action:ty, $id:literal) => {
            $root.on_action(
                cx.listener(|this, _: &$action, window, cx| this.run_command($id, window, cx)),
            )
        };
    }

    let root = command!(root, OpenFile, "file.open");
    let root = command!(root, SaveFile, "file.save");
    let root = command!(root, SaveFileAs, "file.save_as");
    let root = command!(root, SaveAll, "file.save_all");
    let root = command!(root, CloseTab, "file.close_tab");
    let root = command!(root, ZoomIn, "view.zoom_in");
    let root = command!(root, ZoomOut, "view.zoom_out");
    let root = command!(root, ZoomReset, "view.zoom_reset");
    let root = command!(root, ToolSelect, "tools.select");
    let root = command!(root, ToolHighlight, "tools.highlight");
    let root = command!(root, ToolUnderline, "tools.underline");
    let root = command!(root, ToolErase, "tools.erase");
    let root = command!(root, UndoMark, "edit.undo_annotation");
    let root = command!(root, CopySelection, "edit.copy");
    let root = command!(root, SelectAll, "edit.select_all");
    let root = command!(root, ExportPages, "file.export");
    let root = command!(root, OpenSettings, "edit.settings");
    let root = command!(root, ToggleTheme, "view.theme");
    let root = command!(root, GoToPage, "view.go_to_page");

    root.on_action(cx.listener(|this, _: &NextTab, _, cx| {
        this.cycle_tab(1);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &PrevTab, _, cx| {
        this.cycle_tab(-1);
        cx.notify();
    }))
    // Escape unwinds whatever is topmost: a dialog, then a menu, then the page
    // box, then a drag.
    .on_action(cx.listener(|this, _: &Dismiss, _, cx| {
        let handled = this.dismiss_modal()
            || this.close_context_menu()
            || this.title_bar.menu.close()
            || std::mem::take(&mut this.color_dropdown_open)
            || this.close_page_input();
        if !handled {
            if let Some(tab) = this.active_tab_mut() {
                tab.cancel_drag();
            }
        }
        cx.notify();
    }))
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        let modal_open = self.modal_is_open();

        // A selection dragged to the edge of the viewport pulls the column
        // along behind it, and has to keep pulling while the pointer is held
        // still — which produces no events at all. Asking for the next frame
        // is what turns a held pointer into continuous movement.
        if self.step_drag_scroll() {
            window.request_animation_frame();
        }

        // The keyboard half of modality: while a dialog is up — or while the
        // page box is being typed into — focus and the key context both move
        // there, so "Workspace"-scoped bindings, which is every shortcut in
        // the app, stop resolving. Digits reaching the page box instead of
        // Ctrl-less nothing is the same problem a dialog has.
        let (context, focus): (&str, &FocusHandle) = if modal_open {
            ("Modal", &self.modal_focus)
        } else if self.page_input.is_some() {
            ("PageInput", &self.page_focus)
        } else {
            ("Workspace", &self.focus_handle)
        };

        if !focus.is_focused(window) {
            window.focus(focus);
        }

        let title_bar = TitleBar::new(self.window_title(), MENUS)
            .mark("R")
            .style(palette.titlebar_style())
            .menus_enabled(!modal_open)
            .render(&self.title_bar, window, cx);

        let menu_open = self.title_bar.menu.is_open()
            || self.color_dropdown_open
            || self.context_menu.is_some();
        let has_tabs = !self.tabs.is_empty();

        let root = div()
            .key_context(context)
            .track_focus(focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(palette.background))
            .text_color(rgb(palette.text))
            .font_family(UI_FONT)
            .text_size(px(13.))
            // Dragging — the scrollbar's thumb, or a text selection — has to
            // keep working wherever the pointer goes, including outside the
            // element it started in. The window as a whole follows it, and the
            // scrollbar follows it too for the strip the window can't see
            // under it.
            .on_mouse_move(cx.listener(Workspace::pointer_moved))
            .on_mouse_up(MouseButton::Left, cx.listener(Workspace::pointer_released))
            // GPUI delivers a release to whatever is under the pointer, and a
            // drag dropped past the bottom of the window has nothing under it
            // — so the release would never arrive and the column would creep
            // on by itself. This is the same handler for the case where the
            // pointer is outside every hitbox; the two are exclusive, and it
            // is written to be harmless if that ever stops being true.
            .on_mouse_up_out(MouseButton::Left, cx.listener(Workspace::pointer_released));

        let root = with_actions(root, cx)
            // Before the bar, so the deferred dropdowns paint over it.
            .when(menu_open, |root| root.child(self.menu_backdrop(cx)))
            .child(title_bar)
            .when(has_tabs, |root| root.child(self.tab_strip(palette, cx)))
            .child(self.toolbar(palette, cx));

        root.child(self.viewer(palette, window, cx))
            .child(self.status_bar(palette, cx))
            .when_some(
                self.context_menu_panel(palette, window, cx),
                |root, menu| root.child(menu),
            )
            .when(modal_open, |root| {
                root.child(self.modal_layer(palette, window, cx))
            })
    }
}
