//! The page column's scrollbar.
//!
//! GPUI's `overflow_y_scroll` scrolls but draws nothing, so a document is only
//! as navigable as the wheel makes it — which for a five-hundred-page book is
//! not navigable at all. This is a real scrollbar: it shows where in the
//! document you are, and dragging it crosses the whole thing in one gesture.
//!
//! It is an overlay, a sibling of the scroll container rather than a child, so
//! it neither scrolls away with the pages nor narrows the column the pages are
//! centred in. The geometry it draws and the geometry a drag reads are the same
//! [`Tab::scrollbar`] measurement, so the thumb can't drift from the pointer.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, MouseButton, MouseDownEvent, Window};

use crate::app::Workspace;
use crate::tab::{SCROLLBAR_INSET, SCROLLBAR_WIDTH};
use crate::theme::Palette;

impl Workspace {
    /// The scrollbar, or an empty element when the document already fits.
    pub fn page_scrollbar(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(bar) = self.active_tab().and_then(|tab| tab.scrollbar()) else {
            return div().into_any_element();
        };

        let dragging = self.scroll_drag.is_some();

        div()
            .id("page-scrollbar")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(SCROLLBAR_WIDTH))
            .bg(rgb(palette.surface))
            .border_l_1()
            .border_color(rgb(palette.border))
            // Clicks belong to the scrollbar, not to the page behind it — but
            // the wheel still reaches the page, so hovering the bar doesn't
            // stop you scrolling the ordinary way.
            .block_mouse_except_scroll()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::scrollbar_mouse_down))
            // Both drags are also tracked on the window root, which covers the
            // rest of the window; this pair is what keeps them live while the
            // pointer is over the bar itself, where the root is blocked.
            .on_mouse_move(cx.listener(Self::pointer_moved))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::pointer_released))
            .child(
                div()
                    .id("page-scrollbar-thumb")
                    .absolute()
                    .top(px(SCROLLBAR_INSET + bar.thumb_top))
                    .left(px(SCROLLBAR_INSET))
                    .right(px(SCROLLBAR_INSET))
                    .h(px(bar.thumb))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(rgb(if dragging {
                        palette.accent
                    } else {
                        palette.disabled
                    }))
                    .hover(move |style| {
                        style.bg(rgb(if dragging {
                            palette.accent
                        } else {
                            palette.muted
                        }))
                    }),
            )
            .into_any_element()
    }

    fn scrollbar_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A press on the bar also settles whatever else was open, the same way
        // a press on the document does.
        self.title_bar.menu.close();
        self.color_dropdown_open = false;

        self.begin_scroll_drag(f32::from(event.position.y));
        cx.notify();
    }
}
