//! The tab strip. One tab per open document; a dot marks unsaved marks.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, MouseButton, MouseDownEvent, SharedString,
};

use crate::app::Workspace;
use crate::theme::Palette;

impl Workspace {
    pub fn tab_strip(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut strip = div()
            .id("tabs")
            .flex()
            .flex_row()
            .items_end()
            .gap(px(2.))
            .w_full()
            .h(px(30.))
            .px_1()
            .overflow_x_scroll()
            .bg(rgb(palette.surface))
            .border_b_1()
            .border_color(rgb(palette.border))
            .text_size(px(12.));

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active;

            strip = strip.child(
                div()
                    .id(SharedString::from(format!("tab-{index}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(26.))
                    .px_2()
                    .max_w(px(220.))
                    .rounded_t_md()
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .bg(rgb(if is_active {
                        palette.background
                    } else {
                        palette.tab_inactive
                    }))
                    .text_color(rgb(if is_active {
                        palette.text
                    } else {
                        palette.muted
                    }))
                    .hover(move |style| {
                        style.bg(rgb(if is_active {
                            palette.background
                        } else {
                            palette.hover
                        }))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.select_tab(index);
                            cx.notify();
                        }),
                    )
                    .child(div().overflow_hidden().child(tab.title()))
                    .when(tab.is_dirty(), |element| {
                        element.child(
                            div()
                                .w(px(6.))
                                .h(px(6.))
                                .rounded_full()
                                .bg(rgb(palette.accent)),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("tab-close-{index}")))
                            .w(px(16.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_size(px(10.))
                            .hover(move |style| style.bg(rgb(palette.border)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.request_close_tab(index);
                                    cx.notify();
                                }),
                            )
                            .child("✕"),
                    ),
            );
        }

        strip
            .child(
                div()
                    .id("tab-new")
                    .w(px(24.))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(rgb(palette.muted))
                    .hover(move |style| style.bg(rgb(palette.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.open_with_dialog(window, cx);
                            cx.notify();
                        }),
                    )
                    .child("+"),
            )
            .into_any_element()
    }
}
