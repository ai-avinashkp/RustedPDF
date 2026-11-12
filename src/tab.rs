//! One open document, plus everything about how it's being looked at: zoom,
//! scroll position, and the drag in progress.
//!
//! The page column's geometry is computed here rather than measured from
//! elements, so a mouse position can be turned into "page 3, at (210, 604) in
//! PDF points" without needing per-element bounds — which is what the
//! highlighter and the eraser both work in.

use gpui::{point, px, Bounds, Pixels, Point, ScrollHandle};

use crate::pdf::{Area, Document, Span};
use crate::state::settings::{MAX_ZOOM, MIN_ZOOM};

/// Space around and between pages, in logical pixels.
pub const PAGE_GAP: f32 = 16.0;

/// Width of the page column's scrollbar, in logical pixels. Wide enough to
/// put a pointer on without aiming.
pub const SCROLLBAR_WIDTH: f32 = 14.0;
/// Space between the scrollbar's track and its thumb.
pub const SCROLLBAR_INSET: f32 = 2.0;
/// Shortest the thumb is allowed to get. In a five-hundred-page book an
/// honestly proportioned thumb is two pixels tall — too small to see, let
/// alone grab — so past this point it stops shrinking and only the mapping
/// from thumb position to scroll offset keeps scaling.
const MINIMUM_THUMB: f32 = 28.0;

/// How close to the top or bottom edge of the viewport a drag has to get
/// before the page column starts creeping after it, in logical pixels.
const AUTOSCROLL_MARGIN: f32 = 28.0;
/// Speed of that creep at the moment it starts, in logical pixels per second.
const AUTOSCROLL_MINIMUM: f32 = 90.0;
/// And the fastest it will go, however far past the edge the pointer is.
const AUTOSCROLL_MAXIMUM: f32 = 3_000.0;
/// Pixels per second gained for every pixel the pointer is past the margin.
const AUTOSCROLL_RAMP: f32 = 14.0;

/// Zoom stops for the +/- buttons and the View menu.
pub const ZOOM_STOPS: &[f32] = &[0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Read: drag scrolls, nothing is marked.
    Select,
    Highlight,
    Underline,
    /// Click a mark to remove it.
    Erase,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Highlight => "Highlight",
            Tool::Underline => "Underline",
            Tool::Erase => "Eraser",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            // An I-beam, not an arrow: this tool selects text, and an
            // arrow suggested the point-and-click of a drawing app.
            Tool::Select => "⌶",
            Tool::Highlight => "✎",
            Tool::Underline => "_",
            // A blank block, like the end of a pencil eraser.
            Tool::Erase => "▭",
        }
    }
}

/// A selection drag in progress: where it was anchored and where it has got
/// to, each a page and a point in PDF points on it.
///
/// Both ends carry a page because a drag is free to leave the page it started
/// on — which is what selecting a paragraph that runs over a page break
/// means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub anchor: (usize, (f32, f32)),
    pub head: (usize, (f32, f32)),
}

/// How fast the page column should creep after a drag that has run to the edge
/// of the viewport, in logical pixels per second. Negative is upwards.
///
/// Nothing happens until the pointer is within [`AUTOSCROLL_MARGIN`] of an
/// edge, so an ordinary selection well inside the page never drifts under the
/// hand. Past that the speed ramps with how far out the pointer is, which is
/// what lets one gesture both nudge down by a line and cross a whole page —
/// the same way a text field behaves everywhere else.
pub fn autoscroll_velocity(pointer_y: f32, top: f32, bottom: f32) -> f32 {
    // On a viewport too short to hold two margins the zones would overlap and
    // the top would always win, so they give way rather than fight.
    let margin = AUTOSCROLL_MARGIN.min((bottom - top) / 3.);

    let past = if pointer_y < top + margin {
        pointer_y - (top + margin)
    } else if pointer_y > bottom - margin {
        pointer_y - (bottom - margin)
    } else {
        return 0.;
    };

    (AUTOSCROLL_MINIMUM + past.abs() * AUTOSCROLL_RAMP).min(AUTOSCROLL_MAXIMUM) * past.signum()
}

/// A finished text selection, kept so it can be copied.
#[derive(Debug, Clone)]
pub struct Selection {
    pub spans: Vec<Span>,
    pub text: String,
    /// Whether the selection is drawn. Text just highlighted or underlined is
    /// kept as a selection so Ctrl+C and the context menu still act on it,
    /// but painting the selection fill over it would hide the very colour
    /// that was just applied.
    pub shown: bool,
}

impl Selection {
    /// The areas that fall on one page, for drawing it. Nothing for a
    /// selection that isn't shown.
    pub fn areas_on(&self, page: usize) -> &[Area] {
        if !self.shown {
            return &[];
        }
        self.spans
            .iter()
            .find(|span| span.page == page)
            .map_or(&[], |span| span.areas.as_slice())
    }
}

/// Where the page column's scrollbar and its thumb are, in logical pixels.
///
/// `thumb_top` is measured from the top of the *track*, which is where the
/// element positions it. `window_top` is that same origin in window
/// coordinates — a pointer position is in window coordinates, and a drag has
/// to compare the two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scrollbar {
    pub window_top: f32,
    pub thumb_top: f32,
    pub thumb: f32,
    /// How far the thumb can travel: the track's height less the thumb's.
    pub travel: f32,
}

impl Scrollbar {
    /// The thumb for a viewport `viewport` tall with `scrollable` more pixels
    /// of document below it, currently `offset` pixels down.
    ///
    /// `None` when there is nothing to scroll. Pure geometry, so it can be
    /// checked without a window — `window_top` is the caller's to fill in.
    pub fn measure(viewport: f32, scrollable: f32, offset: f32) -> Option<Scrollbar> {
        // Half a pixel of overflow is rounding, not a scrollable document.
        if viewport <= 0. || scrollable <= 0.5 {
            return None;
        }

        let track = (viewport - SCROLLBAR_INSET * 2.).max(1.);
        let visible_fraction = viewport / (viewport + scrollable);
        let thumb = (track * visible_fraction).clamp(MINIMUM_THUMB.min(track), track);
        let travel = (track - thumb).max(0.);

        Some(Scrollbar {
            window_top: 0.,
            thumb_top: (offset / scrollable).clamp(0., 1.) * travel,
            thumb,
            travel,
        })
    }

    /// The scroll offset that puts the thumb's top at `thumb_top` — the
    /// inverse of the mapping [`Scrollbar::measure`] places it with.
    pub fn offset_at(&self, thumb_top: f32, scrollable: f32) -> f32 {
        if self.travel <= 0. {
            return 0.;
        }

        (thumb_top / self.travel).clamp(0., 1.) * scrollable
    }
}

pub struct Tab {
    pub document: Document,
    pub zoom: f32,
    pub scroll: ScrollHandle,
    pub drag: Option<Drag>,
    /// What the drag would mark or select, updated as it moves.
    pub preview: Vec<Span>,
    /// Text selected with the Select tool, waiting to be copied.
    pub selection: Option<Selection>,
    /// Distance from the top of the scrolled content to the top of each page,
    /// in logical pixels, with a final entry holding the total height.
    ///
    /// Every pointer position, every "which pages are on screen" question and
    /// the page counter all need this, several times per frame. Adding the page
    /// heights up on demand made each of those linear in the page count, and
    /// their callers linear again on top: a 500-page book spent its whole frame
    /// budget summing page heights. It only changes when the zoom does.
    tops: Vec<f32>,
    /// Width of the widest page, in logical pixels.
    widest: f32,
}

impl Tab {
    pub fn new(document: Document, zoom: f32) -> Self {
        let mut tab = Self {
            document,
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            scroll: ScrollHandle::new(),
            drag: None,
            preview: Vec::new(),
            selection: None,
            tops: Vec::new(),
            widest: 0.,
        };
        tab.measure();
        tab
    }

    /// Recomputes the page column's geometry for the current zoom.
    fn measure(&mut self) {
        let count = self.document.page_count();

        self.tops = Vec::with_capacity(count + 1);
        self.widest = 0.;

        let mut top = PAGE_GAP;
        for index in 0..count {
            self.tops.push(top);
            let size = self.document.page_size(index);
            self.widest = self.widest.max(size.width * self.zoom);
            top += size.height * self.zoom + PAGE_GAP;
        }
        // The sentinel: one past the last page is the column's full height.
        self.tops.push(top);
    }

    pub fn title(&self) -> String {
        self.document.file_name()
    }

    pub fn is_dirty(&self) -> bool {
        self.document.dirty
    }

    /// Sets zoom, clamped to the supported range. Returns whether it changed,
    /// so the caller knows to drop the rasterised pages.
    pub fn set_zoom(&mut self, zoom: f32) -> bool {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - self.zoom).abs() < 0.001 {
            return false;
        }
        self.zoom = zoom;
        self.measure();
        // The cached bitmaps aren't dropped: each one knows the scale it was
        // rendered at, so `page_image` re-renders it anyway — and until then a
        // pinch can keep stretching it.
        true
    }

    /// Zooms to `zoom` keeping the point under `anchor` (a window position)
    /// where it is on screen — what a pinch or Ctrl+wheel is expected to do.
    /// Zooming about the top of the viewport instead makes the text you were
    /// pointing at slide away from under the fingers.
    pub fn zoom_at(&mut self, zoom: f32, anchor: Point<Pixels>) -> bool {
        let bounds: Bounds<Pixels> = self.scroll.bounds();
        let count = self.document.page_count();
        if count == 0 || bounds.size.width <= Pixels::ZERO {
            return self.set_zoom(zoom);
        }

        let local_x = f32::from(anchor.x - bounds.origin.x);
        let local_y = f32::from(anchor.y - bounds.origin.y);
        let content_x = local_x - f32::from(self.scroll.offset().x);
        let content_y = local_y - f32::from(self.scroll.offset().y);

        // Where the anchor is, in page points — which is the one thing a zoom
        // doesn't change.
        let page = self.page_containing(content_y).min(count - 1);
        let viewport_width = f32::from(bounds.size.width);
        let old_left = self.page_left(page, viewport_width.max(self.content_width()));
        let page_x = (content_x - old_left) / self.zoom;
        let page_y = (content_y - self.page_top(page)) / self.zoom;

        if !self.set_zoom(zoom) {
            return false;
        }

        let new_left = self.page_left(page, viewport_width.max(self.content_width()));
        let new_x = new_left + page_x * self.zoom - local_x;
        let new_y = self.page_top(page) + page_y * self.zoom - local_y;

        // Clamped against the new geometry: the scroll handle's own maximum
        // still describes the old zoom until the next layout.
        let max_x = (self.content_width() - viewport_width).max(0.);
        let max_y = (self.content_height() - f32::from(bounds.size.height)).max(0.);
        self.scroll.set_offset(point(
            px(-new_x.clamp(0., max_x)),
            px(-new_y.clamp(0., max_y)),
        ));
        true
    }

    pub fn zoom_step(&mut self, direction: i32) -> bool {
        let current = self.zoom;
        let next = if direction > 0 {
            ZOOM_STOPS
                .iter()
                .find(|stop| **stop > current + 0.001)
                .copied()
                .unwrap_or(MAX_ZOOM)
        } else {
            ZOOM_STOPS
                .iter()
                .rev()
                .find(|stop| **stop < current - 0.001)
                .copied()
                .unwrap_or(MIN_ZOOM)
        };
        self.set_zoom(next)
    }

    // --- page column geometry --------------------------------------------

    /// Size of a page on screen, in logical pixels.
    pub fn page_display_size(&self, index: usize) -> (f32, f32) {
        let size = self.document.page_size(index);
        (size.width * self.zoom, size.height * self.zoom)
    }

    /// Distance from the top of the scrolled content to the top of a page.
    ///
    /// One past the last page answers with the column's total height, which is
    /// what the viewer's trailing spacer needs.
    pub fn page_top(&self, index: usize) -> f32 {
        self.tops
            .get(index)
            .copied()
            .unwrap_or_else(|| self.content_height())
    }

    /// Full height of the page column, in logical pixels.
    pub fn content_height(&self) -> f32 {
        self.tops.last().copied().unwrap_or(PAGE_GAP)
    }

    /// Left edge of the page column: centred when the page is narrower than the
    /// viewport, otherwise flush to the gutter.
    pub fn page_left(&self, index: usize, viewport_width: f32) -> f32 {
        let width = self.page_display_size(index).0;
        ((viewport_width - width) / 2.).max(PAGE_GAP)
    }

    /// Widest page on screen — the scrolled content is at least this wide, so
    /// wide pages can be scrolled to rather than clipped.
    pub fn content_width(&self) -> f32 {
        self.widest + PAGE_GAP * 2.
    }

    /// Index of the page whose band contains `y`, measured from the top of the
    /// scrolled content; the page count if `y` is past the end.
    ///
    /// `tops` ascends, so this is the last page starting at or above `y` — the
    /// gap below a page counts as part of that page, which is what every
    /// caller wants.
    fn page_containing(&self, y: f32) -> usize {
        self.tops
            .partition_point(|top| *top <= y)
            .saturating_sub(1)
            .min(self.document.page_count())
    }

    /// Which pages to rasterise: those on screen, plus one either side so a
    /// scroll doesn't land on a blank page.
    pub fn visible_pages(&self) -> std::ops::Range<usize> {
        let count = self.document.page_count();
        let viewport = self.scroll.bounds().size.height;

        // Before the first layout there's no viewport to measure against.
        if viewport <= Pixels::ZERO {
            return 0..count.min(2);
        }

        let last_page = count.saturating_sub(1);
        let top = f32::from(-self.scroll.offset().y).max(0.);
        let bottom = top + f32::from(viewport);

        let first = self.page_containing(top).min(last_page);
        let last = self.page_containing(bottom).min(last_page);

        first.saturating_sub(1)..(last + 2).min(count)
    }

    // --- scrolling ---------------------------------------------------------

    /// Puts the top of `index` at the top of the viewport, gap included.
    pub fn scroll_to_page(&self, index: usize) {
        self.scroll_to_offset(self.page_top(index) - PAGE_GAP);
    }

    /// Scrolls to `offset` logical pixels down the column.
    ///
    /// GPUI's offsets run the other way — zero at the top, negative going down
    /// — and it clamps them itself on the next layout; this clamps too, so the
    /// page counter and the scrollbar agree within the same frame.
    pub fn scroll_to_offset(&self, offset: f32) {
        let maximum = f32::from(self.scroll.max_offset().height);
        // Before the first layout there is no maximum yet, and clamping to it
        // would pin every jump to the top of the document.
        let offset = if maximum > 0. {
            offset.clamp(0., maximum)
        } else {
            offset.max(0.)
        };

        self.scroll
            .set_offset(point(self.scroll.offset().x, px(-offset)));
    }

    /// How far down the column the viewport currently is.
    pub fn scroll_offset(&self) -> f32 {
        f32::from(-self.scroll.offset().y).max(0.)
    }

    /// The furthest down it can go. Zero before the first layout, and for a
    /// document that fits without scrolling.
    pub fn max_scroll_offset(&self) -> f32 {
        f32::from(self.scroll.max_offset().height).max(0.)
    }

    /// The scrollbar's geometry, or `None` when the document fits and there is
    /// nothing to scroll.
    pub fn scrollbar(&self) -> Option<Scrollbar> {
        let bounds = self.scroll.bounds();

        let mut bar = Scrollbar::measure(
            f32::from(bounds.size.height),
            f32::from(self.scroll.max_offset().height),
            f32::from(-self.scroll.offset().y),
        )?;

        bar.window_top = f32::from(bounds.origin.y) + SCROLLBAR_INSET;
        Some(bar)
    }

    /// Scrolls so the thumb's top lands at `thumb_top` within the track.
    pub fn scroll_to_thumb(&self, thumb_top: f32) {
        let Some(bar) = self.scrollbar() else { return };
        let scrollable = f32::from(self.scroll.max_offset().height);

        self.scroll_to_offset(bar.offset_at(thumb_top, scrollable));
    }

    /// The page currently filling most of the viewport — what the status bar
    /// calls "page 4 of 12".
    pub fn current_page(&self) -> usize {
        let top = f32::from(-self.scroll.offset().y).max(0.);
        let middle = top + f32::from(self.scroll.bounds().size.height) / 2.;

        self.page_containing(middle)
            .min(self.document.page_count().saturating_sub(1))
    }

    // --- coordinate conversion -------------------------------------------

    /// Turns a window position into a page index and a point in PDF user space
    /// (points, origin bottom-left), or `None` if it isn't over a page.
    pub fn page_at(&self, position: Point<Pixels>) -> Option<(usize, (f32, f32))> {
        let bounds: Bounds<Pixels> = self.scroll.bounds();
        if bounds.size.width <= Pixels::ZERO {
            return None;
        }

        // Position within the scrolled content.
        let x = f32::from(position.x - bounds.origin.x - self.scroll.offset().x);
        let y = f32::from(position.y - bounds.origin.y - self.scroll.offset().y);
        let viewport_width = f32::from(bounds.size.width).max(self.content_width());

        for index in 0..self.document.page_count() {
            let (width, height) = self.page_display_size(index);
            let top = self.page_top(index);
            let left = self.page_left(index, viewport_width);

            if y < top || y > top + height {
                continue;
            }
            if x < left || x > left + width {
                return None;
            }

            return Some((index, self.point_on_page(index, position)));
        }

        None
    }

    /// Where a window position falls on `page`, in PDF user space, clamped to
    /// that page's own box.
    ///
    /// Unlike [`Tab::page_at`] this never fails. Once a drag is under way the
    /// pointer can be anywhere — past the edge of the page, out of the
    /// viewport, over the toolbar — and the selection still has to follow it
    /// to the nearest point on the page it started on, rather than stopping
    /// dead the moment the pointer leaves.
    pub fn point_on_page(&self, page: usize, position: Point<Pixels>) -> (f32, f32) {
        let bounds: Bounds<Pixels> = self.scroll.bounds();
        let viewport_width = f32::from(bounds.size.width).max(self.content_width());

        let x = f32::from(position.x - bounds.origin.x - self.scroll.offset().x);
        let y = f32::from(position.y - bounds.origin.y - self.scroll.offset().y);

        let size = self.document.page_size(page);
        let left = self.page_left(page, viewport_width);
        let top = self.page_top(page);

        // Screen y grows downwards, PDF y grows upwards.
        (
            ((x - left) / self.zoom).clamp(0., size.width),
            (size.height - (y - top) / self.zoom).clamp(0., size.height),
        )
    }

    /// The page and point a drag has reached, for any pointer position at all.
    ///
    /// [`Tab::page_at`] gives up when the pointer isn't over a page — in the
    /// gap between two, past the side of one, or off the viewport entirely.
    /// A drag can't give up: wherever the pointer has got to the selection has
    /// to run to the nearest place in the document, which is what makes
    /// dragging off the bottom edge carry on selecting rather than stop.
    pub fn drag_target(&self, position: Point<Pixels>) -> (usize, (f32, f32)) {
        let bounds: Bounds<Pixels> = self.scroll.bounds();
        let y = f32::from(position.y - bounds.origin.y - self.scroll.offset().y);

        let page = self
            .page_containing(y)
            .min(self.document.page_count().saturating_sub(1));

        (page, self.point_on_page(page, position))
    }

    /// Where an area sits inside its page element, in logical pixels:
    /// `(left, top, width, height)`.
    pub fn area_on_page(&self, page: usize, area: &Area) -> (f32, f32, f32, f32) {
        let size = self.document.page_size(page);
        (
            area.left * self.zoom,
            (size.height - area.top) * self.zoom,
            area.width().max(0.5) * self.zoom,
            area.height().max(0.5) * self.zoom,
        )
    }

    // --- selection --------------------------------------------------------

    pub fn begin_drag(&mut self, page: usize, point: (f32, f32)) {
        self.drag = Some(Drag {
            anchor: (page, point),
            head: (page, point),
        });
        self.preview.clear();
        self.selection = None;
    }

    /// Extends the drag and recomputes what it would mark. Returns whether the
    /// preview changed.
    pub fn extend_drag(&mut self, head: (usize, (f32, f32))) -> bool {
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        if drag.head == head {
            return false;
        }

        drag.head = head;
        let (anchor, head) = (drag.anchor, drag.head);
        self.preview = self.document.selection_spans(anchor, head);
        true
    }

    /// Ends the drag, handing back what it covered, page by page.
    pub fn finish_drag(&mut self) -> Option<Vec<Span>> {
        self.drag.take()?;
        let spans = std::mem::take(&mut self.preview);
        (!spans.is_empty()).then_some(spans)
    }

    /// Ends a Select-tool drag, keeping what it covered — including the text,
    /// so it can be copied. Returns how many characters were selected.
    pub fn finish_text_selection(&mut self) -> Option<usize> {
        let drag = self.drag.take()?;
        let spans = std::mem::take(&mut self.preview);
        if spans.is_empty() {
            return None;
        }

        let text = self.document.selection_text(drag.anchor, drag.head);
        let length = text.chars().count();
        self.selection = Some(Selection {
            spans,
            text,
            shown: true,
        });
        Some(length)
    }

    /// Selects every line of text in the document. Returns how many
    /// characters that is, or `None` for a document with no text at all.
    pub fn select_all(&mut self) -> Option<usize> {
        self.drag = None;
        self.preview.clear();

        let (spans, text) = self.document.select_all();
        if spans.is_empty() {
            self.selection = None;
            return None;
        }

        let length = text.chars().count();
        self.selection = Some(Selection {
            spans,
            text,
            shown: true,
        });
        Some(length)
    }

    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.preview.clear();
        self.selection = None;
    }

    /// Drops the finished selection without disturbing a drag — what Copy does
    /// once the text is on the clipboard.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }
}
