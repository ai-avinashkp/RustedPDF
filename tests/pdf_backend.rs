//! End-to-end checks against real PDFium: rasterising, text selection, and the
//! adopt → edit → save → re-adopt round trip that marks depend on.
//!
//! These need `vendor/pdfium/<library>` to be present; without it every test
//! fails at `Document::open`, which is the intended signal.
//!
//! PDFium is not thread-safe, and the app only ever calls it from the UI
//! thread. `serialised()` reproduces that here — without it the default
//! multi-threaded harness segfaults inside the library.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn serialised() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use pdfium_render::prelude::*;

// The crate is a binary, so pull the modules in directly.
#[path = "../src/pdf/mod.rs"]
mod pdf;

use pdf::mark::MarkKind;
use pdf::Document;

/// Writes a one-page PDF with a line of selectable text.
fn fixture(name: &str) -> PathBuf {
    fixture_with_pages(name, 1)
}

fn fixture_with_pages(name: &str, count: usize) -> PathBuf {
    let path = std::env::temp_dir().join(name);

    let pdfium = pdf::engine::pdfium().expect("PDFium should be available");
    let mut document = pdfium.create_new_pdf().unwrap();

    for number in 0..count {
        let font = document.fonts_mut().helvetica();
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();

        page.objects_mut()
            .create_text_object(
                PdfPoints::new(72.),
                PdfPoints::new(700.),
                format!("Hello RustedPdf {}", number + 1),
                font,
                PdfPoints::new(18.),
            )
            .unwrap();
        page.regenerate_content().unwrap();
    }

    document.save_to_file(&path).unwrap();
    path
}

/// The areas a single-page drag covers. Selection spans pages now; every test
/// below is about one, so this unwraps that case rather than repeating it.
fn areas_on(
    document: &mut Document,
    page: usize,
    from: (f32, f32),
    to: (f32, f32),
) -> Vec<pdf::Area> {
    let spans = document.selection_spans((page, from), (page, to));
    assert!(
        spans.len() <= 1,
        "a drag inside one page stays on it: {spans:?}"
    );
    spans
        .into_iter()
        .next()
        .map(|span| span.areas)
        .unwrap_or_default()
}

#[test]
fn opens_and_rasterises_a_page() {
    let _serial = serialised();
    let path = fixture("rustedpdf-render.pdf");
    let mut document = Document::open(&path).expect("fixture should open");

    assert_eq!(document.page_count(), 1);

    let size = document.page_size(0);
    assert!(
        (size.width - 595.).abs() < 2.,
        "A4 width, got {}",
        size.width
    );

    let image = document
        .page_image(0, 1.5)
        .expect("page should rasterise at 150%");
    let rendered = image.size(0);
    assert_eq!(u32::from(rendered.width), (size.width * 1.5).round() as u32);
}

#[test]
fn selects_text_between_two_points() {
    let _serial = serialised();
    let path = fixture("rustedpdf-select.pdf");
    let mut document = Document::open(&path).unwrap();

    assert!(document.page_has_text(0));

    // Drag across the line of text: one line in, one area out.
    let areas = areas_on(&mut document, 0, (70., 705.), (200., 705.));
    assert_eq!(areas.len(), 1, "one line of text selected: {areas:?}");
    assert!(
        areas[0].width() > 10.,
        "selection has width: {:?}",
        areas[0]
    );
}

#[test]
fn marks_round_trip_through_the_file() {
    let _serial = serialised();
    let path = fixture("rustedpdf-marks.pdf");

    let color = 0x4aa8ff;
    {
        let mut document = Document::open(&path).unwrap();
        assert!(document.marks.is_empty(), "fixture starts clean");

        let areas = areas_on(&mut document, 0, (70., 705.), (200., 705.));
        document.add_marks(0, &areas, MarkKind::Highlight, color, 1.0);
        assert!(document.dirty);

        document.save().expect("save should succeed");
        assert!(!document.dirty, "saving clears the dirty flag");
    }

    // Reopening adopts what was written: same page, kind and colour.
    let reopened = Document::open(&path).unwrap();
    assert_eq!(reopened.marks.len(), 1, "mark survived the round trip");

    let mark = &reopened.marks[0];
    assert_eq!(mark.page, 0);
    assert_eq!(mark.kind, MarkKind::Highlight);
    assert_eq!(mark.color, color);
    assert!(!reopened.dirty, "adopted marks aren't unsaved changes");
}

#[test]
fn selection_yields_the_text_for_copying() {
    let _serial = serialised();
    let path = fixture("rustedpdf-copy.pdf");
    let mut document = Document::open(&path).unwrap();

    let text = document.selection_text((0, (70., 705.)), (0, (300., 705.)));
    assert!(
        text.contains("Hello") && text.contains("RustedPdf"),
        "selected text reads back: {text:?}"
    );
}

#[test]
fn a_new_highlight_replaces_the_one_it_covers() {
    let _serial = serialised();
    let path = fixture("rustedpdf-override.pdf");
    let mut document = Document::open(&path).unwrap();

    let areas = areas_on(&mut document, 0, (70., 705.), (300., 705.));
    document.add_marks(0, &areas, MarkKind::Highlight, 0xffd400, 1.0);
    assert_eq!(document.marks.len(), 1);

    // Highlighting the same words again leaves one mark, in the new colour —
    // stacked highlights would darken the text twice over.
    document.add_marks(0, &areas, MarkKind::Highlight, 0x4aa8ff, 1.0);
    assert_eq!(document.marks.len(), 1, "the old highlight was replaced");
    assert_eq!(document.marks[0].color, 0x4aa8ff);

    // An underline is a different kind of mark, so it coexists.
    document.add_marks(0, &areas, MarkKind::Underline, 0xe5484d, 1.0);
    assert_eq!(document.marks.len(), 2);
}

#[test]
fn highlighting_part_of_a_highlight_keeps_the_rest() {
    let _serial = serialised();
    let path = fixture("rustedpdf-partial.pdf");
    let mut document = Document::open(&path).unwrap();

    // Highlight the whole line…
    let whole = areas_on(&mut document, 0, (70., 705.), (300., 705.));
    document.add_marks(0, &whole, MarkKind::Highlight, 0xffd400, 1.0);
    let full = document.marks[0].area;

    // …then re-highlight a slice out of its middle in another colour.
    let middle = pdf::Area::new(
        full.left + full.width() * 0.4,
        full.bottom,
        full.left + full.width() * 0.6,
        full.top,
    );
    document.add_marks(0, &[middle], MarkKind::Highlight, 0x4aa8ff, 1.0);

    // The old highlight survives either side of the new one rather than being
    // wiped: left piece, new piece, right piece.
    assert_eq!(document.marks.len(), 3, "{:?}", document.marks);

    let new_color = 0x4aa8ff;
    let kept: Vec<_> = document
        .marks
        .iter()
        .filter(|mark| mark.color == 0xffd400)
        .collect();
    assert_eq!(kept.len(), 2, "both ends of the original are still there");
    assert!(kept.iter().all(|mark| mark.area.width() > 0.));
    assert!(document.marks.iter().any(|mark| mark.color == new_color));

    // And the pieces don't overlap the new mark.
    assert!(kept.iter().all(|mark| !mark.area.intersects(&middle)));
}

#[test]
fn eraser_removes_only_the_mark_under_the_pointer() {
    let _serial = serialised();
    let path = fixture("rustedpdf-erase.pdf");
    let mut document = Document::open(&path).unwrap();

    let areas = areas_on(&mut document, 0, (70., 705.), (200., 705.));
    document.add_marks(0, &areas, MarkKind::Highlight, 0xffd400, 1.0);
    let area = document.marks[0].area;

    // A point well away from the text hits nothing.
    assert_eq!(document.remove_marks_at(0, 10., 100.), 0);
    assert_eq!(document.marks.len(), 1);

    // The middle of the mark hits it.
    let (x, y) = ((area.left + area.right) / 2., area.center_y());
    assert_eq!(document.remove_marks_at(0, x, y), 1);
    assert!(document.marks.is_empty());
}

#[test]
fn clearing_part_of_a_mark_keeps_the_rest() {
    let _serial = serialised();
    let path = fixture("rustedpdf-clear-part.pdf");
    let mut document = Document::open(&path).unwrap();

    let line = areas_on(&mut document, 0, (70., 705.), (200., 705.));
    document.add_marks(0, &line, MarkKind::Highlight, 0xffd400, 1.0);
    let whole = document.marks[0].area;

    // Take a chunk out of the middle of the line.
    let middle = areas_on(&mut document, 0, (110., 705.), (150., 705.));
    assert_eq!(document.clear_marks_in(0, &middle), 1);

    assert_eq!(document.marks.len(), 2, "a piece either side survives");
    assert!(document
        .marks
        .iter()
        .all(|mark| mark.area.left >= whole.left && mark.area.right <= whole.right));
    assert!(document.dirty);

    // Clearing where nothing is marked touches nothing.
    assert_eq!(document.clear_marks_in(0, &middle), 0);
}

#[test]
fn exports_one_page_as_a_png() {
    let _serial = serialised();
    let path = fixture("rustedpdf-export-png.pdf");
    let mut document = Document::open(&path).unwrap();

    let width = document.page_size(0).width;
    let rendered = document.render_pages_for_export(&[0], 2.0);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].width, (width * 2.0).round() as u32);

    // The writer pulls each page as it needs it, rather than being handed
    // them all at once.
    let destination = std::env::temp_dir().join("rustedpdf-export.png");
    pdf::export::write_images(&[0], &destination, |index| {
        document.render_page_for_export(index, 2.0)
    })
    .unwrap();

    let written = std::fs::read(&destination).unwrap();
    assert_eq!(&written[..4], b"\x89PNG", "a real PNG was written");
}

#[test]
fn exports_several_pages_as_a_zip() {
    let _serial = serialised();
    let path = fixture_with_pages("rustedpdf-export-zip.pdf", 3);
    let mut document = Document::open(&path).unwrap();

    let rendered = document.render_pages_for_export(&[0, 2], 1.0);
    assert_eq!(rendered.len(), 2, "only the chosen pages are rendered");

    let mut asked_for = Vec::new();
    let destination = std::env::temp_dir().join("rustedpdf-export.zip");
    pdf::export::write_images(&[0, 2], &destination, |index| {
        asked_for.push(index);
        document.render_page_for_export(index, 1.0)
    })
    .unwrap();

    assert_eq!(asked_for, vec![0, 2], "only the chosen pages are rendered");

    let written = std::fs::read(&destination).unwrap();
    assert_eq!(&written[..2], b"PK", "several pages come back as a zip");
}

#[test]
fn exports_chosen_pages_as_a_new_pdf() {
    let _serial = serialised();
    let path = fixture_with_pages("rustedpdf-export-pdf.pdf", 4);
    let mut document = Document::open(&path).unwrap();
    assert_eq!(document.page_count(), 4);

    // Mark a page so the export has something to carry over.
    let areas = areas_on(&mut document, 1, (70., 705.), (300., 705.));
    document.add_marks(1, &areas, MarkKind::Highlight, 0xffd400, 1.0);

    let destination = std::env::temp_dir().join("rustedpdf-export-subset.pdf");
    document
        .export_pages_as_pdf(&[1, 3], &destination)
        .expect("export should succeed");

    let exported = Document::open(&destination).unwrap();
    assert_eq!(exported.page_count(), 2, "only the chosen pages");
    assert_eq!(exported.marks.len(), 1, "the mark came with its page");
    assert_eq!(exported.marks[0].page, 0, "and moved to its new index");

    // The source document is left as it was — its marks are still ours, not
    // annotations baked into it.
    assert_eq!(document.marks.len(), 1);
}

#[test]
fn rejects_files_that_are_not_pdfs() {
    let _serial = serialised();
    let path = std::env::temp_dir().join("rustedpdf-not-a.pdf");
    std::fs::write(&path, b"this is not a PDF").unwrap();

    let message = match Document::open(&path) {
        Ok(_) => panic!("should refuse a non-PDF"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("not a PDF"),
        "message names the problem: {message}"
    );
}

#[test]
fn a_selection_runs_across_a_page_break() {
    let _serial = serialised();
    let path = fixture_with_pages("rustedpdf-cross-page.pdf", 3);
    let mut document = Document::open(&path).unwrap();

    // Drag from the line on page 1 to the line on page 3. Reading order says
    // that covers the rest of page 1, all of page 2, and the start of page 3.
    let spans = document.selection_spans((0, (70., 705.)), (2, (300., 705.)));

    assert_eq!(spans.len(), 3, "one span per page touched: {spans:?}");
    assert_eq!(
        spans.iter().map(|span| span.page).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "and in reading order"
    );
    assert!(spans.iter().all(|span| !span.areas.is_empty()));

    // The text comes back page by page, in order, one line each.
    let text = document.selection_text((0, (70., 705.)), (2, (300., 705.)));
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        vec![
            "Hello RustedPdf 1",
            "Hello RustedPdf 2",
            "Hello RustedPdf 3"
        ],
        "got {text:?}"
    );
}

#[test]
fn dragging_back_up_selects_the_same_run() {
    let _serial = serialised();
    let path = fixture_with_pages("rustedpdf-cross-page-up.pdf", 3);
    let mut document = Document::open(&path).unwrap();

    // Which end you started from is not something a selection should care
    // about — dragging up the page has to give what dragging down gave.
    let down = document.selection_text((0, (70., 705.)), (2, (300., 705.)));
    let up = document.selection_text((2, (300., 705.)), (0, (70., 705.)));
    assert_eq!(down, up);

    let spans = document.selection_spans((2, (300., 705.)), (0, (70., 705.)));
    assert_eq!(
        spans.iter().map(|span| span.page).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn a_page_with_no_text_is_skipped_rather_than_breaking_the_run() {
    let _serial = serialised();

    // Page 2 is blank — a chapter divider, a full-page figure, a scan.
    let path = std::env::temp_dir().join("rustedpdf-cross-page-blank.pdf");
    {
        let pdfium = pdf::engine::pdfium().unwrap();
        let mut document = pdfium.create_new_pdf().unwrap();
        for number in [1, 2, 3] {
            let font = document.fonts_mut().helvetica();
            let mut page = document
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::a4())
                .unwrap();
            if number != 2 {
                page.objects_mut()
                    .create_text_object(
                        PdfPoints::new(72.),
                        PdfPoints::new(700.),
                        format!("Hello RustedPdf {number}"),
                        font,
                        PdfPoints::new(18.),
                    )
                    .unwrap();
            }
            page.regenerate_content().unwrap();
        }
        document.save_to_file(&path).unwrap();
    }

    let mut document = Document::open(&path).unwrap();
    let spans = document.selection_spans((0, (70., 705.)), (2, (300., 705.)));

    // The blank page contributes nothing and is left out, rather than
    // stopping the selection at it.
    assert_eq!(
        spans.iter().map(|span| span.page).collect::<Vec<_>>(),
        vec![0, 2],
        "{spans:?}"
    );
    let text = document.selection_text((0, (70., 705.)), (2, (300., 705.)));
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        vec!["Hello RustedPdf 1", "Hello RustedPdf 3"]
    );
}
