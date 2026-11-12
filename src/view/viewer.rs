//! The page column: rasterised pages, the marks drawn over them, and the
//! pointer handling for the highlighter, the underliner and the eraser.
//!
//! Only the pages in (or next to) the viewport are rasterised — a page bitmap
//! is several megabytes, so [`crate::pdf::Document::trim_page_cache`] drops the
//! rest every frame.
//!
//! They are also the only pages given elements. The rest of the column is two
//! spacers standing in for the runs above and below, sized from the same
//! geometry the pages themselves are laid out with, so the scrollbar and every
//! pointer position stay exactly where they would be if all the pages were
//! there. Building an element per page instead is fine for a leaflet and
//! hopeless for a book: a five-hundred-page document meant five hundred laid
//! out, measured and painted nodes on every single frame.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, img, prelude::*, px, rgb, rgba, AnyElement, Context, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, RenderImage, ScrollWheelEvent, Window,
};

use crate::app::{Workspace, PINCH_SETTLE};
use crate::pdf::MarkKind;
use crate::tab::{Selection, Tool, PAGE_GAP};
use crate::theme::Palette;

impl Workspace {
    pub fn viewer(
        &mut self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.tabs.is_empty() {
            return self.start_screen(palette, cx);
        }

        let active = self.active;
        let device_scale = window.scale_factor();

        let (zoom, visible) = {
            let tab = &self.tabs[active];
            (tab.zoom, tab.visible_pages())
        };

        // Rasterise first, so the element tree can be built from shared
        // borrows. Only the visible run is asked for, and only it is kept:
        // both caches are trimmed to what the next few frames will want.
        // The pages a drag is anchored to and pointing at are kept in the
        // character cache however far the viewport has wandered from them.
        let drag_pages: Vec<usize> = self.tabs[active]
            .drag
            .map_or_else(Vec::new, |drag| vec![drag.anchor.0, drag.head.0]);

        // Mid-pinch, the bitmaps already rendered are stretched to the new
        // size rather than re-rendered on every step; see
        // `Document::page_image_or_stale`.
        let pinching = self.is_pinching();

        let images: Vec<Option<Arc<RenderImage>>> = {
            let tab = &mut self.tabs[active];
            let images = visible
                .clone()
                .map(|index| {
                    if pinching {
                        tab.document.page_image_or_stale(index, zoom * device_scale)
                    } else {
                        tab.document.page_image(index, zoom * device_scale)
                    }
                })
                .collect();
            tab.document.trim_page_cache(visible.clone());
            tab.document.trim_char_cache(visible.clone(), &drag_pages);
            images
        };

        let tab = &self.tabs[active];

        // The runs of pages that aren't being drawn, as their exact heights.
        // `page_top` already accounts for the gap above each page, and the
        // column's top padding is the gap above the first.
        let above = tab.page_top(visible.start) - PAGE_GAP;
        let below = tab.content_height() - tab.page_top(visible.end);

        let mut column = div()
            // The scroll container is a flex box: without `flex_none` the
            // column is stretched to the viewport and the pages ride out of
            // view above it.
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            // Spacing is a bottom margin on each page rather than a flex gap,
            // so the spacers below can carry their own run's gaps and the two
            // stay in step with `Tab`'s geometry.
            .pt(px(PAGE_GAP))
            // The gutter either side is what `Tab::page_left` assumes. Without
            // it a page wider than the viewport sat flush against the left
            // edge while the pointer maths put it a gutter further right, and
            // every drag at a high zoom landed a few characters off.
            .px(px(PAGE_GAP))
            .min_w_full()
            // At least as tall as the viewport, so a pinch over the empty
            // space below a short document still reaches the handler below.
            .min_h_full()
            // Ctrl+wheel, and a touchpad pinch — which Windows delivers as
            // exactly that — zoom about the pointer. Handled here on the
            // column rather than on the scroll container: a child's handler
            // runs first, so stopping it here keeps the container from also
            // scrolling by the same event.
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if !event.modifiers.control {
                    return;
                }
                cx.stop_propagation();

                let amount = f32::from(event.delta.pixel_delta(window.line_height()).y);
                if amount == 0. || !this.pinch_zoom(amount, event.position) {
                    return;
                }

                // Render the pages sharp once the fingers stop. Every step
                // arms its own wake-up; the ones that land mid-pinch find it
                // still going and change nothing.
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(PINCH_SETTLE + Duration::from_millis(20))
                        .await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
                cx.notify();
            }))
            .child(div().flex_none().h(px(above.max(0.))));

        // The selection is drawn in the selection colour; a highlighter or
        // underliner drag is drawn as what it will leave behind, in the mark
        // colour, so what you see while dragging is what you get on release.
        let selection_fill = rgba(palette.selection << 8 | 0x55);
        let mark_preview = self.mark_kind();
        let mark_alpha = (self.settings.mark_opacity * 0x70 as f32).round() as u32;
        let mark_fill = rgba(self.settings.mark_color << 8 | mark_alpha.clamp(0x18, 0xff));
        let rule_fill = rgb(self.settings.mark_color);

        for index in visible.clone() {
            let (width, height) = tab.page_display_size(index);

            let mut page = div()
                .relative()
                .flex_none()
                .w(px(width))
                .h(px(height))
                .mb(px(PAGE_GAP))
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(rgb(palette.border))
                .shadow_md()
                .overflow_hidden();

            page = match images[index - visible.start].clone() {
                Some(image) => page.child(img(image).w(px(width)).h(px(height))),
                // A page PDFium wouldn't give us keeps its size, so scrolling
                // stays stable and the rest of the document still reads.
                None => page.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(palette.muted))
                        .child(format!("Page {}", index + 1)),
                ),
            };

            // Marks aren't drawn here: they're multiplied into the page bitmap
            // by `Document::page_image`, so highlighted text stays as crisp as
            // unhighlighted text.

            // Live preview of the drag in progress, and the finished text
            // selection, are transient — those stay as overlays.
            let dragging = tab.drag.is_some();
            let areas: &[_] = if dragging {
                tab.preview
                    .iter()
                    .find(|span| span.page == index)
                    .map_or(&[], |span| span.areas.as_slice())
            } else {
                tab.selection
                    .as_ref()
                    .map_or(&[], |selection| selection.areas_on(index))
            };

            for area in areas {
                let (left, top, area_width, area_height) = tab.area_on_page(index, area);
                let overlay = div().absolute().left(px(left)).w(px(area_width));

                page = page.child(match (dragging, mark_preview) {
                    (true, Some(MarkKind::Highlight)) => {
                        overlay.top(px(top)).h(px(area_height)).bg(mark_fill)
                    }
                    // The same rule `Mark::painted_area` paints: 2pt along
                    // the bottom of the line.
                    (true, Some(MarkKind::Underline)) => overlay
                        .top(px(top + area_height - 1.5 * zoom))
                        .h(px((2. * zoom).max(1.)))
                        .bg(rule_fill),
                    _ => overlay.top(px(top)).h(px(area_height)).bg(selection_fill),
                });
            }

            column = column.child(page);
        }

        column = column.child(div().flex_none().h(px(below.max(0.))));

        let pages = div()
            .id("viewer")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_start()
            .overflow_y_scroll()
            .overflow_x_scroll()
            .track_scroll(&tab.scroll)
            .bg(rgb(palette.background))
            // Every tool but the eraser works by dragging across text, so the
            // I-beam is the honest cursor for them.
            .when(self.tool != Tool::Erase, |viewer| viewer.cursor_text())
            .when(self.tool == Tool::Erase, |viewer| viewer.cursor_pointer())
            // Only the press is handled here — it is the one event that has to
            // hit-test a page. Moves and releases are handled at the window
            // root, because a drag that has run off the bottom of the viewport
            // is precisely the case that matters, and the column stops hearing
            // about the pointer the moment it leaves.
            .on_mouse_down(MouseButton::Left, cx.listener(Self::viewer_mouse_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    // A drag still in progress has nothing sensible to do
                    // with a menu opening over it.
                    if this
                        .tabs
                        .get(this.active)
                        .is_some_and(|tab| tab.drag.is_some())
                    {
                        return;
                    }
                    this.open_context_menu(event.position);
                    cx.notify();
                }),
            )
            .child(column);

        // The scrollbar overlays the pages rather than sitting beside them:
        // laid out as a sibling it would narrow the column, and the pages
        // would shift sideways the moment a document became long enough to
        // scroll.
        div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(pages)
            .child(self.page_scrollbar(palette, cx))
            .into_any_element()
    }

    fn viewer_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A click in the document also dismisses whatever the toolbar had
        // open, the page box included.
        self.title_bar.menu.close();
        self.close_page_input();
        self.close_context_menu();

        // The eraser works anywhere — a stroke can start beside the page and
        // sweep onto it — so it doesn't wait for the hit test below.
        if self.tool == Tool::Erase {
            self.begin_erase(event.position);
            cx.notify();
            return;
        }

        let tool = self.tool;
        let active = self.active;
        let Some(tab) = self.tabs.get_mut(active) else {
            return;
        };
        let Some((page, point)) = tab.page_at(event.position) else {
            return;
        };

        match tool {
            // Select drags out a text selection, which Ctrl+C copies.
            Tool::Select => {
                if tab.document.page_has_text(page) {
                    tab.begin_drag(page, point);
                } else {
                    tab.selection = None;
                }
            }
            // Handled above.
            Tool::Erase => {}
            Tool::Highlight | Tool::Underline => {
                if tab.document.page_has_text(page) {
                    tab.begin_drag(page, point);
                } else {
                    self.status =
                        format!("Page {} has no selectable text to mark", page + 1).into();
                }
            }
        }

        cx.notify();
    }

    /// Every pointer move in the window, whatever it is over.
    ///
    /// Both drags the app has — the scrollbar's and the selection's — outlive
    /// the element that started them, so both are followed from here.
    pub(crate) fn pointer_moved(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.continue_scroll_drag(f32::from(event.position.y));
        changed |= self.continue_text_drag(event.position);
        changed |= self.continue_erase(event.position);

        if changed {
            cx.notify();
        }
    }

    /// Every left release in the window. Harmless when nothing is being
    /// dragged: both drags end by taking state that isn't there.
    pub(crate) fn pointer_released(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Letting go stops the viewport creeping, whatever else happens next.
        self.drag_scroll = None;
        if self.end_scroll_drag() {
            cx.notify();
        }
        if self.end_erase() {
            cx.notify();
            return;
        }
        let Some(kind) = self.mark_kind() else {
            // Select tool: keep what was dragged over as a text selection.
            let selected = self
                .tabs
                .get_mut(self.active)
                .and_then(|tab| tab.finish_text_selection());

            if let Some(characters) = selected {
                self.status = format!("Selected {characters} characters — Ctrl+C to copy").into();
                cx.notify();
            }
            return;
        };

        let color = self.settings.mark_color;
        let opacity = self.settings.mark_opacity;
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let drag = tab.drag;
        let Some(spans) = tab.finish_drag() else {
            return;
        };

        // Keep what was marked as the selection too, so Ctrl+C copies the text
        // you just highlighted without having to drag it out again — but
        // undrawn, or the selection fill would sit over the new colour and
        // hide it until you clicked somewhere else.
        if let Some(drag) = drag {
            let text = tab.document.selection_text(drag.anchor, drag.head);
            tab.selection = Some(Selection {
                spans: spans.clone(),
                text,
                shown: false,
            });
        }

        // Marks are still made one page at a time — they are stored against a
        // page, and written back as annotations on it. A selection that
        // crossed a page break simply becomes one mark run per page.
        let added: usize = spans
            .iter()
            .map(|span| {
                tab.document
                    .add_marks(span.page, &span.areas, kind, color, opacity)
            })
            .sum();

        self.status = match (added, spans.len()) {
            (0, _) => "Nothing to mark — drag across some text".into(),
            (1, _) => format!("{} added", kind.label()).into(),
            (count, 1) => format!("{} added over {count} lines", kind.label()).into(),
            (count, pages) => {
                format!("{} added over {count} lines on {pages} pages", kind.label()).into()
            }
        };

        self.record_journal();
        cx.notify();
    }
}
