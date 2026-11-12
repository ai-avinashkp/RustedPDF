//! The right-click menu over the document.
//!
//! Every row is a command id, run through [`Workspace::run_command`] like the
//! menu bar and the keyboard, so a context-menu action can't behave
//! differently from the same action reached any other way.

use gpui::{
    deferred, div, prelude::*, px, rgb, AnyElement, Context, MouseButton, MouseDownEvent,
    SharedString, Window,
};

use crate::app::Workspace;
use crate::tab::Tool;
use crate::theme::Palette;
use crate::ui::titlebar::MENU_LAYER;

const WIDTH: f32 = 236.;
const ROW_HEIGHT: f32 = 26.;
const SEPARATOR_HEIGHT: f32 = 9.;
const PADDING: f32 = 5.;

enum Row {
    Command {
        id: &'static str,
        label: &'static str,
        shortcut: &'static str,
        enabled: bool,
        /// Drawn with a tick — the tool in hand.
        checked: bool,
    },
    Separator,
}

fn command(id: &'static str, label: &'static str, shortcut: &'static str, enabled: bool) -> Row {
    Row::Command {
        id,
        label,
        shortcut,
        enabled,
        checked: false,
    }
}

impl Workspace {
    fn context_rows(&self) -> Vec<Row> {
        let selected = self.context_menu_has_selection();
        let tool = |id, label, shortcut, which: Tool| Row::Command {
            id,
            label,
            shortcut,
            enabled: true,
            checked: self.tool == which,
        };

        let mut rows = vec![
            command("edit.copy", "Copy", "Ctrl+C", selected),
            command("edit.select_all", "Select All", "Ctrl+A", true),
            Row::Separator,
            command(
                "edit.highlight_selection",
                "Highlight Selection",
                "",
                selected,
            ),
            command(
                "edit.underline_selection",
                "Underline Selection",
                "",
                selected,
            ),
            command(
                "edit.clear_selection_marks",
                "Remove Marks in Selection",
                "",
                selected,
            ),
        ];
        if self.context_menu_has_mark() {
            rows.push(command(
                "edit.remove_mark_here",
                "Remove Mark Here",
                "",
                true,
            ));
        }
        rows.extend([
            command("edit.undo_annotation", "Undo Last Mark", "Ctrl+Z", true),
            Row::Separator,
            tool("tools.select", "Select", "Ctrl+1", Tool::Select),
            tool("tools.highlight", "Highlighter", "Ctrl+2", Tool::Highlight),
            tool("tools.underline", "Underline", "Ctrl+3", Tool::Underline),
            tool("tools.erase", "Eraser", "Ctrl+4", Tool::Erase),
            Row::Separator,
            command("view.zoom_in", "Zoom In", "Ctrl++", true),
            command("view.zoom_out", "Zoom Out", "Ctrl+-", true),
            command("view.zoom_reset", "Actual Size", "Ctrl+0", true),
        ]);
        rows
    }

    /// The menu, if one is open — painted above everything but a dialog.
    pub fn context_menu_panel(
        &self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu?;
        let rows = self.context_rows();

        // Kept inside the window: a menu opened near the bottom or right edge
        // opens upwards or leftwards instead of running off it.
        let height = PADDING * 2.
            + rows
                .iter()
                .map(|row| match row {
                    Row::Separator => SEPARATOR_HEIGHT,
                    Row::Command { .. } => ROW_HEIGHT,
                })
                .sum::<f32>();
        let viewport = window.viewport_size();
        let (x, y) = (f32::from(menu.position.x), f32::from(menu.position.y));
        let left = if x + WIDTH > f32::from(viewport.width) {
            (x - WIDTH).max(0.)
        } else {
            x
        };
        let top = if y + height > f32::from(viewport.height) {
            (y - height).max(0.)
        } else {
            y
        };

        let mut panel = div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .py(px(PADDING))
            .rounded_md()
            .bg(rgb(palette.panel))
            .border_1()
            .border_color(rgb(palette.border))
            .shadow_lg()
            .text_color(rgb(palette.text))
            .text_size(px(13.))
            // Clicks inside the menu must not reach the dismiss backdrop.
            .occlude();

        for (index, row) in rows.into_iter().enumerate() {
            panel = panel.child(match row {
                Row::Separator => div()
                    .h(px(SEPARATOR_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.)).w_full().bg(rgb(palette.border)))
                    .into_any_element(),
                Row::Command {
                    id,
                    label,
                    shortcut,
                    enabled,
                    checked,
                } => div()
                    .id(SharedString::from(format!("context-row-{index}")))
                    .h(px(ROW_HEIGHT))
                    .mx_1()
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .rounded_sm()
                    .whitespace_nowrap()
                    .text_color(rgb(if enabled {
                        palette.text
                    } else {
                        palette.disabled
                    }))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |style| style.bg(rgb(palette.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.run_command(id, window, cx);
                                }),
                            )
                    })
                    .child(format!("{}{label}", if checked { "✓  " } else { "    " }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(palette.muted))
                            .child(shortcut),
                    )
                    .into_any_element(),
            });
        }

        Some(deferred(panel).with_priority(MENU_LAYER).into_any_element())
    }
}
