//! What's shown with nothing open: the recent list, which doubles as the
//! recent-files manager.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, MouseButton, MouseDownEvent, SharedString,
};

use crate::app::Workspace;
use crate::pdf::engine;
use crate::theme::Palette;
use crate::view::widgets::{button, divider, section_heading};

impl Workspace {
    pub fn start_screen(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let engine_missing = !engine::is_available();

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(palette.background))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(84.))
                            .h(px(108.))
                            .rounded_md()
                            .border_2()
                            .border_dashed()
                            .border_color(rgb(palette.border))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(24.))
                            .text_color(rgb(palette.muted))
                            .child("PDF"),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .text_color(rgb(palette.text))
                            .child("No document open"),
                    )
                    .child(
                        button("start-open", "Open PDF…", palette, true).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                this.open_with_dialog(window, cx);
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .when(engine_missing, |screen| {
                screen.child(
                    div()
                        .max_w(px(520.))
                        .p_3()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(palette.danger))
                        .text_color(rgb(palette.text))
                        .child(
                            "PDFium could not be loaded, so documents can't be opened. \
                             See vendor/pdfium/README.md.",
                        ),
                )
            })
            .child(self.recent_list(palette, cx))
            .into_any_element()
    }

    /// The recent list. Entries are pruned when their file disappears, so
    /// anything shown here can actually be opened.
    fn recent_list(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.recents.files.is_empty() {
            return div().into_any_element();
        }

        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(520.))
            .p_3()
            .rounded_md()
            .bg(rgb(palette.panel))
            .border_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(section_heading("RECENT FILES", palette))
                    .child(
                        div()
                            .id("recent-clear")
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(rgb(palette.muted))
                            .hover(move |style| style.text_color(rgb(palette.text)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.recents.clear();
                                    this.status = "Cleared the recent list".into();
                                    cx.notify();
                                }),
                            )
                            .child("Clear"),
                    ),
            )
            .child(divider(palette));

        for (index, entry) in self.recents.files.iter().enumerate() {
            let path = entry.path.clone();
            let forget_path = entry.path.clone();

            list = list.child(
                div()
                    .id(SharedString::from(format!("recent-{index}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py(px(4.))
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(palette.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                            this.open_path(&path, window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child(div().text_color(rgb(palette.text)).child(entry.file_name()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(palette.muted))
                                    .overflow_hidden()
                                    .child(entry.directory()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(palette.muted))
                                    .child(format!("{} · {} pages", entry.age(), entry.page_count)),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("recent-forget-{index}")))
                                    .w(px(18.))
                                    .h(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .text_size(px(10.))
                                    .text_color(rgb(palette.muted))
                                    .hover(move |style| style.bg(rgb(palette.border)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.recents.remove(&forget_path);
                                            this.status = "Removed from the recent list".into();
                                            cx.notify();
                                        }),
                                    )
                                    .child("✕"),
                            ),
                    ),
            );
        }

        list.into_any_element()
    }
}
