//! The page column's geometry.
//!
//! [`Tab`] caches the top of every page rather than adding heights up on
//! demand, and the viewer draws only the pages on screen, standing the rest of
//! the column up as two spacers measured from that same cache. Both of those
//! are only correct if the cache agrees with the layout the pages themselves
//! produce — a spacer a few pixels out puts every page under the pointer at
//! the wrong offset — so this pins the arithmetic down, and the scrollbar
//! mapping that reads the same column with it.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use pdfium_render::prelude::*;

// The crate is a binary, so pull the modules in directly. `tab` reaches for
// `crate::pdf` and `crate::state`, which is why both are mounted here too.
#[path = "../src/pdf/mod.rs"]
mod pdf;
#[path = "../src/state/mod.rs"]
mod state;
#[path = "../src/tab.rs"]
mod tab;

use pdf::Document;
use tab::{autoscroll_velocity, Scrollbar, Tab, PAGE_GAP, SCROLLBAR_INSET};

/// PDFium is not thread safe, and the app only ever calls it from the UI
/// thread; the default multi-threaded harness needs the same discipline.
fn serialised() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An A4 document of `count` blank pages.
fn fixture(name: &str, count: usize) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    let pdfium = pdf::engine::pdfium().expect("PDFium should be available");
    let mut document = pdfium.create_new_pdf().unwrap();

    for _ in 0..count {
        document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();
    }

    document.save_to_file(&path).unwrap();
    path
}

fn tab_of(name: &str, pages: usize, zoom: f32) -> Tab {
    let document = Document::open(&fixture(name, pages)).expect("fixture should open");
    Tab::new(document, zoom)
}

/// The height the viewer lays out for one page, gap included.
fn stride(tab: &Tab, index: usize) -> f32 {
    tab.page_display_size(index).1 + PAGE_GAP
}

#[test]
fn page_tops_step_by_one_page_and_one_gap() {
    let _serial = serialised();
    let tab = tab_of("rustedpdf-geometry.pdf", 6, 1.0);

    // The first page sits below the column's top padding.
    assert_eq!(tab.page_top(0), PAGE_GAP);

    for index in 0..tab.document.page_count() {
        let expected = tab.page_top(index) + stride(&tab, index);
        assert!(
            (tab.page_top(index + 1) - expected).abs() < 0.001,
            "page {} starts at {}, expected {expected}",
            index + 1,
            tab.page_top(index + 1)
        );
    }
}

#[test]
fn content_height_is_one_past_the_last_page() {
    let _serial = serialised();
    let tab = tab_of("rustedpdf-geometry-height.pdf", 6, 1.0);
    let count = tab.document.page_count();

    // The sentinel and the total are the same number: the viewer's trailing
    // spacer is `content_height() - page_top(end)`, and it has to come out at
    // zero when nothing follows the last page.
    assert!((tab.page_top(count) - tab.content_height()).abs() < 0.001);
    assert!((tab.page_top(count + 5) - tab.content_height()).abs() < 0.001);

    let laid_out = PAGE_GAP + (0..count).map(|index| stride(&tab, index)).sum::<f32>();
    assert!(
        (tab.content_height() - laid_out).abs() < 0.001,
        "column is {} tall, the pages lay out to {laid_out}",
        tab.content_height()
    );
}

#[test]
fn spacers_and_pages_add_up_to_the_whole_column() {
    let _serial = serialised();
    let tab = tab_of("rustedpdf-geometry-spacers.pdf", 12, 1.0);

    // Whatever run of pages the viewer decides to draw, the two spacers plus
    // the drawn pages have to be exactly as tall as the column claims to be.
    for (start, end) in [(0, 12), (0, 3), (4, 7), (9, 12), (5, 5)] {
        let above = tab.page_top(start) - PAGE_GAP;
        let drawn = (start..end).map(|index| stride(&tab, index)).sum::<f32>();
        let below = tab.content_height() - tab.page_top(end);

        let total = PAGE_GAP + above + drawn + below;
        assert!(
            (total - tab.content_height()).abs() < 0.001,
            "drawing pages {start}..{end} gives a {total}px column, not {}",
            tab.content_height()
        );
        assert!(above >= 0. && below >= 0., "spacers can't be negative");
    }
}

#[test]
fn zooming_rescales_the_column() {
    let _serial = serialised();
    let mut tab = tab_of("rustedpdf-geometry-zoom.pdf", 5, 1.0);

    let single = tab.content_height();
    let width = tab.content_width();
    assert!(tab.set_zoom(2.0), "zoom changed");

    // Pages double; the gaps between them don't, so the column is short of
    // twice as tall by exactly the gaps that were counted once too often.
    let gaps = PAGE_GAP * (tab.document.page_count() + 1) as f32;
    assert!(
        (tab.content_height() - (single * 2. - gaps)).abs() < 0.01,
        "at 200% the column is {}, expected {}",
        tab.content_height(),
        single * 2. - gaps
    );

    // Width is one page wide plus a gutter either side, so it scales the same
    // way — and the cache has to have been rebuilt for either to be true.
    assert!((tab.content_width() - (width * 2. - PAGE_GAP * 2.)).abs() < 0.01);
}

// --- the scrollbar ---------------------------------------------------------
//
// `Tab::scrollbar` needs a laid-out window to read the viewport from, which a
// test doesn't have — but the arithmetic it defers to doesn't, and that is the
// part that can be wrong. A thumb that walks off the end of its track, or that
// doesn't land back where the pointer put it, is a scrollbar that fights you.

/// A page of an A4 book on a laptop screen: 800px of viewport, 500 pages.
const VIEWPORT: f32 = 800.;
const LONG_BOOK: f32 = 430_000.;

fn track_of(viewport: f32) -> f32 {
    viewport - SCROLLBAR_INSET * 2.
}

#[test]
fn there_is_no_thumb_when_there_is_nothing_to_scroll() {
    assert!(Scrollbar::measure(VIEWPORT, 0., 0.).is_none());
    // Sub-pixel overflow is rounding, not a scrollable document.
    assert!(Scrollbar::measure(VIEWPORT, 0.4, 0.).is_none());
    // Nor before the first layout, when there is no viewport yet.
    assert!(Scrollbar::measure(0., LONG_BOOK, 0.).is_none());

    assert!(Scrollbar::measure(VIEWPORT, 1., 0.).is_some());
}

#[test]
fn the_thumb_stays_inside_its_track() {
    let track = track_of(VIEWPORT);

    for scrollable in [1., 200., 5_000., LONG_BOOK] {
        for fraction in [0., 0.001, 0.5, 0.999, 1.] {
            let bar = Scrollbar::measure(VIEWPORT, scrollable, scrollable * fraction)
                .expect("scrollable");

            assert!(bar.thumb > 0. && bar.thumb <= track, "thumb {}", bar.thumb);
            assert!(bar.thumb_top >= 0., "thumb_top {}", bar.thumb_top);
            assert!(
                bar.thumb_top + bar.thumb <= track + 0.001,
                "thumb ends at {}, past a {track}px track",
                bar.thumb_top + bar.thumb
            );
        }
    }
}

#[test]
fn a_long_book_still_gets_a_grabbable_thumb() {
    // Proportionally this thumb is a pixel and a half tall. It has to stop
    // shrinking somewhere above "impossible to hit".
    let bar = Scrollbar::measure(VIEWPORT, LONG_BOOK, 0.).expect("scrollable");
    assert!(bar.thumb >= 20., "thumb is only {}px tall", bar.thumb);
    assert!(bar.travel > 0., "and it can still travel");
}

#[test]
fn the_ends_of_the_track_are_the_ends_of_the_document() {
    let track = track_of(VIEWPORT);

    let top = Scrollbar::measure(VIEWPORT, LONG_BOOK, 0.).unwrap();
    assert_eq!(top.thumb_top, 0.);

    let bottom = Scrollbar::measure(VIEWPORT, LONG_BOOK, LONG_BOOK).unwrap();
    assert!(
        (bottom.thumb_top + bottom.thumb - track).abs() < 0.001,
        "at the end of the document the thumb should be at the end of the track"
    );

    // Past the end — the offset gpui hands back mid-clamp — doesn't overshoot.
    let past = Scrollbar::measure(VIEWPORT, LONG_BOOK, LONG_BOOK * 2.).unwrap();
    assert_eq!(past.thumb_top, bottom.thumb_top);
}

#[test]
fn dragging_the_thumb_lands_where_it_was_put() {
    // The drag maps a thumb position back to a scroll offset; drawing maps
    // that offset back to a thumb position. Round-tripping is what stops the
    // thumb drifting away from the pointer over a long drag.
    let bar = Scrollbar::measure(VIEWPORT, LONG_BOOK, 0.).unwrap();

    for thumb_top in [0., 1., bar.travel / 3., bar.travel / 2., bar.travel] {
        let offset = bar.offset_at(thumb_top, LONG_BOOK);
        let redrawn = Scrollbar::measure(VIEWPORT, LONG_BOOK, offset).unwrap();

        assert!(
            (redrawn.thumb_top - thumb_top).abs() < 0.01,
            "grabbed at {thumb_top}, came back at {}",
            redrawn.thumb_top
        );
    }

    // Dragging past either end pins to it rather than running away.
    assert_eq!(bar.offset_at(-500., LONG_BOOK), 0.);
    assert_eq!(bar.offset_at(bar.travel + 500., LONG_BOOK), LONG_BOOK);
}

// --- auto-scroll while dragging a selection --------------------------------
//
// The speed curve decides whether dragging past the edge feels like a text
// field or like a fight. It is pure arithmetic, so it can be pinned down here;
// what can't is the drag itself, which needs a pointer.

/// A viewport occupying window rows 100..800 — the page column under a
/// titlebar, a tab strip and a toolbar.
const TOP: f32 = 100.;
const BOTTOM: f32 = 800.;

#[test]
fn a_selection_inside_the_viewport_does_not_drift() {
    // The whole point: reading and selecting in the middle of the page must
    // not set the document sliding under the hand.
    for y in [TOP + 40., 300., 450., BOTTOM - 40.] {
        assert_eq!(
            autoscroll_velocity(y, TOP, BOTTOM),
            0.,
            "pointer at {y} is well inside the viewport"
        );
    }
}

#[test]
fn nearing_an_edge_pulls_the_column_that_way() {
    // Up is negative, down is positive — the same sign convention the scroll
    // offset uses.
    assert!(autoscroll_velocity(TOP + 5., TOP, BOTTOM) < 0.);
    assert!(autoscroll_velocity(TOP - 200., TOP, BOTTOM) < 0.);
    assert!(autoscroll_velocity(BOTTOM - 5., TOP, BOTTOM) > 0.);
    assert!(autoscroll_velocity(BOTTOM + 200., TOP, BOTTOM) > 0.);
}

#[test]
fn the_speed_ramps_with_how_far_out_the_pointer_is() {
    // One gesture has to do both jobs: nudge down by a line, and cross a
    // page. That only works if pushing further speeds it up.
    let nudge = autoscroll_velocity(BOTTOM - 10., TOP, BOTTOM);
    let further = autoscroll_velocity(BOTTOM + 50., TOP, BOTTOM);
    let hard = autoscroll_velocity(BOTTOM + 400., TOP, BOTTOM);

    assert!(nudge > 0. && further > nudge && hard > further);

    // It starts at a speed you can actually see, rather than from zero.
    assert!(nudge >= 90., "creep starts at {nudge}px/s");
    // And it stops accelerating, so an overshoot doesn't fire the document
    // off the end.
    assert_eq!(hard, autoscroll_velocity(BOTTOM + 100_000., TOP, BOTTOM));
}

#[test]
fn the_two_edges_are_mirror_images() {
    for offset in [0., 30., 300.] {
        let up = autoscroll_velocity(TOP - offset, TOP, BOTTOM);
        let down = autoscroll_velocity(BOTTOM + offset, TOP, BOTTOM);
        assert!(
            (up + down).abs() < 0.001,
            "{offset}px past the top gives {up}, past the bottom {down}"
        );
    }
}

#[test]
fn a_short_viewport_still_has_a_middle() {
    // With a fixed margin the two zones would overlap on a small window and
    // the top would win everywhere — dragging down would scroll up.
    let (top, bottom) = (0., 60.);

    assert_eq!(
        autoscroll_velocity(30., top, bottom),
        0.,
        "the middle holds"
    );
    assert!(autoscroll_velocity(2., top, bottom) < 0.);
    assert!(
        autoscroll_velocity(58., top, bottom) > 0.,
        "down is still down"
    );
}
