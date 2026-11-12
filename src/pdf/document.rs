//! An open PDF: page geometry, rasterised page cache, text boxes for
//! selection, and the highlight/underline marks layered over it.
//!
//! # How marks and the file relate
//!
//! On open, every highlight/underline annotation already in the file is
//! *adopted*: read into [`Mark`]s and removed from the in-memory document. So
//! the document PDFium renders never contains our marks — the viewer draws them
//! itself, which is what makes the eraser, live colour changes and undo work
//! uniformly for marks made this session and marks loaded from disk.
//!
//! On save the current marks are written back as real PDF annotations, the file
//! is written, and the annotations are stripped again to restore the invariant.
//! Nothing touches the file on disk until an explicit save.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context as _, Result};
use gpui::RenderImage;
use image::{Frame, RgbaImage};
use pdfium_render::prelude::*;

use crate::pdf::engine;
use crate::pdf::export::{page_range_string, RenderedPage};
use crate::pdf::mark::{Area, Mark, MarkKind, Span};

/// A highlight or underline read back out of a file: page, kind, area, colour
/// and opacity.
type AdoptedMark = (usize, MarkKind, Area, u32, f32);

/// Page dimensions in PDF points.
#[derive(Debug, Clone, Copy)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

struct CachedPage {
    /// Device pixels per point this was rasterised at.
    scale: f32,
    /// Mark generation the marks were baked in at.
    revision: u64,
    image: Arc<RenderImage>,
}

/// One character, with where it sits on the page — the basis for both
/// selection geometry and copied text.
struct CharBox {
    area: Area,
    character: char,
}

pub struct Document {
    pub path: PathBuf,
    pub marks: Vec<Mark>,
    /// Marks added, removed or recoloured since the last save.
    pub dirty: bool,
    pages: Vec<PageSize>,
    next_mark_id: u64,
    /// Bumped whenever marks change, so rasterised pages know they're stale.
    marks_revision: u64,
    document: PdfDocument<'static>,
    page_cache: HashMap<usize, CachedPage>,
    /// Per-page character boxes, built on first use — selection is pure
    /// geometry once this exists, so dragging doesn't hit PDFium per frame.
    char_cache: HashMap<usize, Vec<CharBox>>,
    /// Every line on a page, for the pages a selection passes straight over.
    ///
    /// A selection dragged across fifty pages re-reads its own geometry on
    /// every frame, and character boxes are far too heavy to keep fifty pages
    /// of. The pages in the middle of a selection are always covered entirely,
    /// though, so what they contribute never changes — and as lines rather
    /// than characters it is a kilobyte a page instead of sixty.
    line_cache: HashMap<usize, Vec<Area>>,
}

impl Document {
    /// Opens `path`, adopting any highlight/underline annotations it contains.
    pub fn open(path: &Path) -> Result<Self> {
        let pdfium = engine::pdfium()?;

        let bytes =
            std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;

        // Cheap guard so a mis-named file fails with a clear message rather
        // than a PDFium error code.
        if !bytes.starts_with(b"%PDF-") {
            return Err(anyhow!(
                "{} is not a PDF file.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }

        // Loaded from memory rather than from a file handle: PDFium keeps
        // reading from its source for the life of the document, and holding the
        // file open would make saving over the original fail on Windows.
        let document = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(|error| anyhow!("Could not open {}: {error}", path.display()))?;

        let pages = document
            .pages()
            .iter()
            .map(|page| PageSize {
                width: page.width().value,
                height: page.height().value,
            })
            .collect::<Vec<_>>();

        if pages.is_empty() {
            return Err(anyhow!("{} has no pages.", path.display()));
        }

        let mut opened = Self {
            path: path.to_path_buf(),
            marks: Vec::new(),
            dirty: false,
            pages,
            next_mark_id: 1,
            marks_revision: 0,
            document,
            page_cache: HashMap::new(),
            char_cache: HashMap::new(),
            line_cache: HashMap::new(),
        };

        opened.adopt_marks_from_document();
        Ok(opened)
    }

    // --- geometry --------------------------------------------------------

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page_size(&self, index: usize) -> PageSize {
        self.pages.get(index).copied().unwrap_or(PageSize {
            width: 612.,
            height: 792.,
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    // --- rasterisation ---------------------------------------------------

    /// The page as an image at `scale` device pixels per point, with this
    /// page's marks blended in, rendering it if the cache is stale.
    ///
    /// Highlights are *multiplied* into the pixels rather than drawn as
    /// translucent boxes on top. That's what a real highlighter does: white
    /// paper takes the colour, black glyphs stay black. An overlay, however
    /// faint, always washes the text out.
    pub fn page_image(&mut self, index: usize, scale: f32) -> Option<Arc<RenderImage>> {
        if let Some(cached) = self.page_cache.get(&index) {
            if (cached.scale - scale).abs() < 0.001 && cached.revision == self.marks_revision {
                return Some(cached.image.clone());
            }
        }

        let size = *self.pages.get(index)?;
        let target_width = raster_width(size, scale);

        let config = PdfRenderConfig::new()
            .set_target_width(target_width)
            .set_format(PdfBitmapFormat::BGRA)
            // Pages are transparent by default; paper is white.
            .set_clear_color(PdfColor::new(255, 255, 255, 255))
            .render_annotations(true);

        let (width, height, mut bytes) = {
            let page = self.document.pages().get(index as PdfPageIndex).ok()?;
            let bitmap = page.render_with_config(&config).ok()?;
            (
                bitmap.width() as u32,
                bitmap.height() as u32,
                bitmap.as_raw_bytes(),
            )
        };

        // The bitmap's own scale, not the requested one: `raster_width` may
        // have capped it, and the marks have to land where the pixels are.
        blend_marks(
            &mut bytes,
            width,
            height,
            width as f32 / size.width.max(1.),
            size.height,
            self.marks.iter().filter(|mark| mark.page == index),
        );

        // gpui uploads image data as BGRA, which is what we asked PDFium for —
        // the buffer type is nominally Rgba, the byte order is not.
        let buffer = RgbaImage::from_raw(width, height, bytes)?;
        let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));

        self.page_cache.insert(
            index,
            CachedPage {
                scale,
                revision: self.marks_revision,
                image: image.clone(),
            },
        );
        Some(image)
    }

    /// Like [`Document::page_image`], but happy to hand back a bitmap rendered
    /// at another scale, for the viewer to stretch.
    ///
    /// A pinch produces a new zoom on every frame, and rasterising every page
    /// in view at each of them makes the gesture stutter. Mid-pinch a slightly
    /// soft page that follows the fingers beats a sharp one that lags them;
    /// the sharp one is rendered once the fingers stop. Marks still have to be
    /// current — a stale bitmap with last revision's marks would be a lie, not
    /// a blur.
    pub fn page_image_or_stale(&mut self, index: usize, scale: f32) -> Option<Arc<RenderImage>> {
        if let Some(cached) = self.page_cache.get(&index) {
            if cached.revision == self.marks_revision {
                return Some(cached.image.clone());
            }
        }

        self.page_image(index, scale)
    }

    /// Drops rasterised pages outside `keep`. Page bitmaps are megabytes each,
    /// so the viewer calls this with the range it's actually showing.
    pub fn trim_page_cache(&mut self, keep: std::ops::Range<usize>) {
        self.page_cache.retain(|index, _| keep.contains(index));
    }

    /// Drops cached character boxes for pages far from `keep`.
    ///
    /// A page of a dense book is a few thousand characters, so a long reading
    /// session through a 500-page document would otherwise accumulate tens of
    /// megabytes of geometry nobody is going to select again. The window is
    /// generous either side of the viewport so scrolling back a page doesn't
    /// pay to rebuild it.
    /// `pinned` pages are kept whatever the range says. A drag anchored fifty
    /// pages back still needs that page's characters on every frame to know
    /// where the selection starts, and re-reading them each time would cost
    /// more than the whole rest of the frame.
    pub fn trim_char_cache(&mut self, keep: std::ops::Range<usize>, pinned: &[usize]) {
        const MARGIN: usize = 8;
        let first = keep.start.saturating_sub(MARGIN);
        let last = keep.end.saturating_add(MARGIN);

        self.char_cache
            .retain(|index, _| (first..last).contains(index) || pinned.contains(index));
    }

    // --- text selection --------------------------------------------------

    /// Characters on the page, in reading order, with their boxes.
    fn chars(&mut self, index: usize) -> &[CharBox] {
        if !self.char_cache.contains_key(&index) {
            let mut boxes = Vec::new();

            if let Ok(page) = self.document.pages().get(index as PdfPageIndex) {
                if let Ok(text) = page.text() {
                    for character in text.chars().iter() {
                        if let Ok(bounds) = character.loose_bounds() {
                            boxes.push(CharBox {
                                area: area_from(bounds),
                                character: character.unicode_char().unwrap_or(' '),
                            });
                        }
                    }
                }
            }

            self.char_cache.insert(index, boxes);
        }

        &self.char_cache[&index]
    }

    /// The selection between two points, as one [`Span`] per page it touches
    /// and one area per line within each — the shape a highlighter needs.
    ///
    /// This follows reading order rather than the dragged rectangle, so
    /// dragging from the middle of one line to the middle of another selects
    /// the run of text between them, like a text cursor does. Across pages it
    /// means the same thing: the rest of the page you started on, all of any
    /// page in between, and the beginning of the one you stopped on.
    pub fn selection_spans(
        &mut self,
        from: (usize, (f32, f32)),
        to: (usize, (f32, f32)),
    ) -> Vec<Span> {
        let (first, last) = in_reading_order(from, to);
        let mut spans = Vec::new();

        for page in first.0..=last.0.min(self.pages.len().saturating_sub(1)) {
            // A point only bounds the run on the page it was taken on. A page
            // in the middle is covered entirely, and its lines are cached —
            // which is what keeps a fifty-page selection from re-reading fifty
            // pages of characters on every frame.
            let areas = if page == first.0 || page == last.0 {
                let start = (page == first.0).then_some(first.1);
                let end = (page == last.0).then_some(last.1);

                let chars = self.chars(page);
                match char_range(chars, start, end) {
                    Some((a, b)) => group_into_lines(&chars[a..=b]),
                    None => continue,
                }
            } else {
                self.page_lines(page).to_vec()
            };

            if !areas.is_empty() {
                spans.push(Span { page, areas });
            }
        }

        spans
    }

    /// The selected text, with a newline between lines — what Copy puts on the
    /// clipboard.
    pub fn selection_text(&mut self, from: (usize, (f32, f32)), to: (usize, (f32, f32))) -> String {
        let (first, last) = in_reading_order(from, to);
        let mut text = String::new();

        for page in first.0..=last.0.min(self.pages.len().saturating_sub(1)) {
            let start = (page == first.0).then_some(first.1);
            let end = (page == last.0).then_some(last.1);

            let chars = self.chars(page);
            let Some((a, b)) = char_range(chars, start, end) else {
                continue;
            };

            // A page break is a line break. Anything more — a marker, a blank
            // line — would be this app editorialising about text it was asked
            // to copy verbatim.
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&run_of_text(&chars[a..=b]));
        }

        text.trim().to_string()
    }

    /// Every line of text in the document, and all of its text — what Select
    /// All selects.
    pub fn select_all(&mut self) -> (Vec<Span>, String) {
        let mut spans = Vec::new();
        let mut text = String::new();

        for page in 0..self.pages.len() {
            let chars = self.chars(page);
            if chars.is_empty() {
                continue;
            }

            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&run_of_text(chars));

            let areas = self.page_lines(page).to_vec();
            if !areas.is_empty() {
                spans.push(Span { page, areas });
            }
        }

        (spans, text.trim().to_string())
    }

    /// Every line of text on a page, as one area each — what a selection that
    /// runs straight over the page covers.
    fn page_lines(&mut self, page: usize) -> &[Area] {
        if !self.line_cache.contains_key(&page) {
            let lines = group_into_lines(self.chars(page));
            self.line_cache.insert(page, lines);
        }

        &self.line_cache[&page]
    }

    /// True if the page has selectable text at all — a scanned page has none,
    /// and the highlighter would silently do nothing.
    pub fn page_has_text(&mut self, page: usize) -> bool {
        !self.chars(page).is_empty()
    }

    // --- marks -----------------------------------------------------------

    /// Adds marks over `areas`.
    ///
    /// Where a new mark lands on an old one of the same kind, it takes over
    /// *only the part it covers*: the old mark is trimmed back to the words the
    /// new one doesn't touch, rather than being deleted. Highlighting one word
    /// inside a highlighted sentence recolours that word and leaves the rest
    /// alone — and nothing ever stacks, which would darken the text twice over.
    pub fn add_marks(
        &mut self,
        page: usize,
        areas: &[Area],
        kind: MarkKind,
        color: u32,
        opacity: f32,
    ) -> usize {
        let mut trimmed = Vec::with_capacity(self.marks.len());
        for mark in std::mem::take(&mut self.marks) {
            if mark.page != page || mark.kind != kind {
                trimmed.push(mark);
                continue;
            }

            // Whittle the old mark down by each new area in turn.
            let mut remaining = vec![mark.area];
            for area in areas {
                remaining = remaining
                    .iter()
                    .flat_map(|piece| piece.without(area))
                    .collect();
            }

            trimmed.extend(remaining.into_iter().map(|area| Mark { area, ..mark }));
        }
        self.marks = trimmed;

        for area in areas {
            self.marks.push(Mark {
                id: self.next_mark_id,
                page,
                kind,
                area: *area,
                color,
                opacity,
            });
            self.next_mark_id += 1;
        }

        self.marks_revision += 1;
        if !areas.is_empty() {
            self.dirty = true;
        }
        areas.len()
    }

    /// Removes every mark under the point, not just the topmost — what an
    /// eraser being dragged across a page does. Returns how many went.
    pub fn remove_marks_at(&mut self, page: usize, x: f32, y: f32) -> usize {
        let before = self.marks.len();
        self.marks
            .retain(|mark| mark.page != page || !mark.hit_area().contains(x, y));

        let removed = before - self.marks.len();
        if removed > 0 {
            self.marks_revision += 1;
            self.dirty = true;
        }
        removed
    }

    /// Takes marks of either kind off `areas`, the same way a new mark takes
    /// over an old one: only the covered part goes, so un-highlighting one
    /// word of a highlighted sentence leaves the rest of the sentence alone.
    /// Returns how many marks were trimmed or removed.
    pub fn clear_marks_in(&mut self, page: usize, areas: &[Area]) -> usize {
        let mut touched = 0;
        let mut kept = Vec::with_capacity(self.marks.len());

        for mark in std::mem::take(&mut self.marks) {
            if mark.page != page {
                kept.push(mark);
                continue;
            }

            let mut remaining = vec![mark.area];
            for area in areas {
                remaining = remaining
                    .iter()
                    .flat_map(|piece| piece.without(area))
                    .collect();
            }

            if remaining.len() != 1 || remaining[0] != mark.area {
                touched += 1;
            }
            kept.extend(remaining.into_iter().map(|area| Mark { area, ..mark }));
        }
        self.marks = kept;

        if touched > 0 {
            self.marks_revision += 1;
            self.dirty = true;
        }
        touched
    }

    pub fn undo_last_mark(&mut self) -> bool {
        let removed = self.marks.pop().is_some();
        self.marks_revision += 1;
        self.dirty |= removed;
        removed
    }

    pub fn clear_marks_on_page(&mut self, page: usize) -> usize {
        let before = self.marks.len();
        self.marks.retain(|mark| mark.page != page);
        let removed = before - self.marks.len();
        self.marks_revision += 1;
        self.dirty |= removed > 0;
        removed
    }

    pub fn clear_all_marks(&mut self) -> usize {
        let removed = self.marks.len();
        self.marks.clear();
        self.marks_revision += 1;
        self.dirty |= removed > 0;
        removed
    }

    /// Replaces the mark set wholesale — used when recovering a crash journal.
    pub fn restore_marks(&mut self, marks: Vec<Mark>) {
        self.next_mark_id = marks.iter().map(|mark| mark.id).max().unwrap_or(0) + 1;
        self.marks = marks;
        self.marks_revision += 1;
        self.dirty = true;
    }

    // --- saving ----------------------------------------------------------

    // --- exporting -------------------------------------------------------

    /// Rasterises one page for export, marks included — what the viewer shows.
    ///
    /// One page at a time rather than a batch: the export writer encodes and
    /// writes each page before asking for the next, so exporting a whole book
    /// costs one page bitmap rather than all of them.
    pub fn render_page_for_export(&mut self, index: usize, scale: f32) -> Option<RenderedPage> {
        let size = self.pages.get(index).copied()?;

        let config = PdfRenderConfig::new()
            .set_target_width(raster_width(size, scale))
            .set_format(PdfBitmapFormat::BGRA)
            .set_clear_color(PdfColor::new(255, 255, 255, 255))
            .render_annotations(true);

        // The bitmap borrows the page, so both have to stay alive until the
        // bytes are copied out.
        let (width, height, mut bgra) = self
            .document
            .pages()
            .get(index as PdfPageIndex)
            .ok()
            .and_then(|page| {
                let bitmap = page.render_with_config(&config).ok()?;
                Some((
                    bitmap.width() as u32,
                    bitmap.height() as u32,
                    bitmap.as_raw_bytes(),
                ))
            })?;

        // The bitmap's own scale, not the requested one: `raster_width` may
        // have capped it, and the marks have to land where the pixels are.
        let rendered_scale = width as f32 / size.width.max(1.);

        blend_marks(
            &mut bgra,
            width,
            height,
            rendered_scale,
            size.height,
            self.marks.iter().filter(|mark| mark.page == index),
        );

        Some(RenderedPage {
            index,
            width,
            height,
            bgra,
        })
    }

    /// Rasterises several pages at once. Only worth it for a handful — see
    /// [`Document::render_page_for_export`] for why the export path doesn't.
    #[cfg(test)]
    pub fn render_pages_for_export(&mut self, pages: &[usize], scale: f32) -> Vec<RenderedPage> {
        pages
            .iter()
            .filter_map(|index| self.render_page_for_export(*index, scale))
            .collect()
    }

    /// Writes `pages` out as a new PDF, marks and all.
    pub fn export_pages_as_pdf(&self, pages: &[usize], destination: &Path) -> Result<()> {
        if pages.is_empty() {
            return Err(anyhow!("No pages were selected."));
        }

        let pdfium = engine::pdfium()?;

        // Marks only exist in our model, so put them into the document for the
        // duration of the copy — exactly as saving does.
        self.write_marks_into_document()?;

        let result = (|| -> Result<()> {
            let mut exported = pdfium
                .create_new_pdf()
                .map_err(|error| anyhow!("Could not create the PDF: {error}"))?;

            exported
                .pages_mut()
                .copy_pages_from_document(&self.document, &page_range_string(pages), 0)
                .map_err(|error| anyhow!("Could not copy the pages: {error}"))?;

            exported
                .save_to_file(destination)
                .map_err(|error| anyhow!("Could not write {}: {error}", destination.display()))
        })();

        self.strip_marks_from_document();
        result
    }

    /// Writes the marks into the file at `self.path`.
    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone();
        self.write_to(&path)?;
        self.dirty = false;
        Ok(())
    }

    /// Writes a copy elsewhere and continues editing that copy, like Save As.
    pub fn save_as(&mut self, path: &Path) -> Result<()> {
        self.write_to(path)?;
        self.path = path.to_path_buf();
        self.dirty = false;
        Ok(())
    }

    /// Materialise marks → write → strip them again, so the in-memory document
    /// keeps its "no marks of ours" invariant whether or not the write worked.
    ///
    /// The file is written beside the target and moved into place, so a failure
    /// part-way through can't leave a truncated PDF where the original was.
    fn write_to(&self, path: &Path) -> Result<()> {
        self.write_marks_into_document()?;

        let temporary = temporary_path(path);
        let written = self
            .document
            .save_to_file(&temporary)
            .map_err(|error| anyhow!("Could not write {}: {error}", path.display()));

        self.strip_marks_from_document();
        written?;

        std::fs::rename(&temporary, path)
            .with_context(|| format!("Could not replace {}", path.display()))?;
        Ok(())
    }

    fn write_marks_into_document(&self) -> Result<()> {
        for mark in &self.marks {
            let mut page = self
                .document
                .pages()
                .get(mark.page as PdfPageIndex)
                .map_err(|error| anyhow!("Page {} is unavailable: {error}", mark.page + 1))?;

            let color = PdfColor::new(
                (mark.color >> 16) as u8,
                (mark.color >> 8) as u8,
                mark.color as u8,
                mark.alpha(),
            );
            let bounds = rect_from(mark.painted_area());
            let annotations = page.annotations_mut();

            match mark.kind {
                MarkKind::Highlight => {
                    let mut annotation = annotations
                        .create_highlight_annotation()
                        .map_err(|error| anyhow!("Could not add highlight: {error}"))?;
                    let _ = annotation.set_bounds(bounds);
                    let _ = annotation.set_fill_color(color);
                }
                MarkKind::Underline => {
                    let mut annotation = annotations
                        .create_underline_annotation()
                        .map_err(|error| anyhow!("Could not add underline: {error}"))?;
                    let _ = annotation.set_bounds(bounds);
                    let _ = annotation.set_fill_color(color);
                    let _ = annotation.set_stroke_color(color);
                }
            }
        }

        Ok(())
    }

    /// Removes every highlight/underline annotation from the in-memory
    /// document, optionally recording them as marks first.
    ///
    /// Everything goes through `annotations_mut`: the shared accessor hands out
    /// annotations borrowed from the page, which can't outlive the borrow the
    /// deletion needs.
    fn take_marks_from_document(&self, adopt: bool) -> Vec<AdoptedMark> {
        let mut adopted = Vec::new();

        for index in 0..self.pages.len() {
            let Ok(mut page) = self.document.pages().get(index as PdfPageIndex) else {
                continue;
            };
            let annotations = page.annotations_mut();

            // Deleting shifts the indices, so re-scan from the start each time
            // rather than iterating a collection we're mutating.
            loop {
                let found = (0..annotations.len()).find_map(|position| {
                    let annotation = annotations.get(position).ok()?;
                    let kind = match annotation.annotation_type() {
                        PdfPageAnnotationType::Highlight => MarkKind::Highlight,
                        PdfPageAnnotationType::Underline => MarkKind::Underline,
                        _ => return None,
                    };
                    let bounds = annotation.bounds().ok()?;
                    let (color, opacity) = annotation_color(&annotation);
                    Some((position, kind, area_from(bounds), color, opacity))
                });

                let Some((position, kind, area, color, opacity)) = found else {
                    break;
                };

                let Ok(annotation) = annotations.get(position) else {
                    break;
                };
                if annotations.delete_annotation(annotation).is_err() {
                    break;
                }

                if adopt {
                    adopted.push((index, kind, area, color, opacity));
                }
            }
        }

        adopted
    }

    fn adopt_marks_from_document(&mut self) {
        self.marks_revision += 1;
        for (page, kind, area, color, opacity) in self.take_marks_from_document(true) {
            // An underline was stored as the painted rule; put the text-height
            // area back so the eraser and re-save round-trip cleanly.
            let area = match kind {
                MarkKind::Underline => Area {
                    bottom: area.bottom + 0.5,
                    top: area.bottom + 0.5,
                    ..area
                },
                MarkKind::Highlight => area,
            };

            self.marks.push(Mark {
                id: self.next_mark_id,
                page,
                kind,
                area,
                color,
                opacity,
            });
            self.next_mark_id += 1;
        }

        // Adoption isn't an edit: the file already contains these.
        self.dirty = false;
    }

    fn strip_marks_from_document(&self) {
        self.take_marks_from_document(false);
    }
}

// --- helpers -------------------------------------------------------------

/// The colour and strength of an annotation already in the file, as
/// `(0xRRGGBB, opacity)`.
///
/// `PdfPageAnnotationCommon::fill_color` is *not* safe to call here. When the
/// annotation carries an appearance stream — which every highlight written by
/// another PDF app does — `FPDFAnnot_GetColor` returns false, and
/// pdfium-render's fallback then calls `FPDFPageObj_GetFillColor` with the
/// annotation handle cast to a page-object handle. PDFium dereferences it as
/// the wrong type and the process dies with an access violation: opening any
/// document that contained someone else's highlight closed the whole app.
///
/// So the appearance stream's own objects are asked instead. Those are real
/// page objects, the call is the one their handle is for, and their fill
/// colour is what the highlight actually looks like. Only annotations with no
/// appearance stream — the ones RustedPdf writes itself — go through
/// `fill_color`, where the crashing branch is unreachable.
fn annotation_color(annotation: &PdfPageAnnotation) -> (u32, f32) {
    let objects = annotation.objects();

    let color = if objects.len() > 0 {
        objects.iter().find_map(|object| object.fill_color().ok())
    } else {
        annotation.fill_color().ok()
    };

    match color {
        Some(color) => (
            (color.red() as u32) << 16 | (color.green() as u32) << 8 | color.blue() as u32,
            // Alpha was written from the mark's opacity on save.
            (color.alpha() as f32 / 200.).clamp(0.05, 1.),
        ),
        // A mark whose colour can't be read is still a mark: keep it, in the
        // default highlighter yellow, rather than dropping it.
        None => (0xffd400, 1.),
    }
}

/// Target width in device pixels for rasterising a page at `scale`, capped so
/// no single page bitmap can exhaust memory or outgrow what the GPU will take.
///
/// Two limits, both real: a texture wider or taller than [`MAX_RASTER_EDGE`]
/// is refused by most GPUs, and even within that a very tall page at a high
/// zoom is hundreds of megabytes of BGRA. Clamping the width is enough for
/// both, because height follows from the page's aspect ratio.
fn raster_width(size: PageSize, scale: f32) -> i32 {
    /// Conservative floor for `GL_MAX_TEXTURE_SIZE` / D3D equivalents.
    const MAX_RASTER_EDGE: f32 = 8_192.;
    /// 64 MB of BGRA — enough for a page filling a 4K screen, and a ceiling
    /// that a book-sized document can hold several of.
    const MAX_RASTER_PIXELS: f32 = 16_000_000.;

    let width = (size.width * scale).max(1.);
    let height = (size.height * scale).max(1.);

    // Every cap is expressed as a factor to shrink the requested width by, so
    // the page keeps its aspect ratio whichever one bites.
    let by_edge = (MAX_RASTER_EDGE / width.max(height)).min(1.);
    let by_area = (MAX_RASTER_PIXELS / (width * height)).sqrt().min(1.);

    (width * by_edge.min(by_area))
        .round()
        .clamp(1., MAX_RASTER_EDGE) as i32
}

fn area_from(rect: PdfRect) -> Area {
    Area::new(
        rect.left().value,
        rect.bottom().value,
        rect.right().value,
        rect.top().value,
    )
}

fn rect_from(area: Area) -> PdfRect {
    PdfRect::new_from_values(area.bottom, area.left, area.top, area.right)
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.pdf".to_string());
    path.with_file_name(format!(".{name}.rustedpdf-tmp"))
}

/// Orders the two ends of a drag by page, so the rest of the selection code
/// can assume it is walking forwards.
///
/// Within one page the order of the two points doesn't matter —
/// [`char_range`] sorts them by reading order, which is not the same as by
/// position and is the thing that makes dragging upwards work.
fn in_reading_order(
    from: (usize, (f32, f32)),
    to: (usize, (f32, f32)),
) -> ((usize, (f32, f32)), (usize, (f32, f32))) {
    if from.0 <= to.0 {
        (from, to)
    } else {
        (to, from)
    }
}

/// The run of characters between two points, in reading order.
///
/// `None` for either end means the edge of the page: that is what a selection
/// arriving from the page above, or leaving towards the page below, covers.
fn char_range(
    chars: &[CharBox],
    from: Option<(f32, f32)>,
    to: Option<(f32, f32)>,
) -> Option<(usize, usize)> {
    if chars.is_empty() {
        return None;
    }

    let start = from.map_or(0, |(x, y)| nearest_char(chars, x, y));
    let end = to.map_or(chars.len() - 1, |(x, y)| nearest_char(chars, x, y));

    Some(if start <= end {
        (start, end)
    } else {
        (end, start)
    })
}

/// A run of characters as text, with a newline where the line changes.
///
/// Only visible characters with a real box decide where lines break. PDFium
/// gives spaces, and the `\r\n` it generates at line ends, boxes that are
/// zero-height or sit off the line — compared like any other character, each
/// one read as a line of its own, and every word was copied onto a separate
/// line with a lone space between. So whitespace is only noted as "a space
/// goes here", and the line question is asked between the visible characters
/// either side of it.
fn run_of_text(chars: &[CharBox]) -> String {
    let mut text = String::new();
    // The last visible character that had real geometry.
    let mut previous: Option<Area> = None;
    let mut pending_space = false;

    for current in chars {
        let character = current.character;
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }

        let area = current.area;
        let has_geometry = area.width() > 0. && area.height() > 0.;

        if !text.is_empty() {
            match previous {
                Some(before) if has_geometry && !before.shares_line_with(&area) => {
                    text.push('\n');
                }
                Some(before) if has_geometry => {
                    // PDFs often encode spaces as a gap rather than a space
                    // character.
                    let gap = area.left - before.right;
                    if pending_space || gap > before.height() * 0.25 {
                        text.push(' ');
                    }
                }
                _ if pending_space => text.push(' '),
                _ => {}
            }
        }

        text.push(character);
        pending_space = false;
        if has_geometry {
            previous = Some(area);
        }
    }

    text
}

/// Index of the character nearest the point — the anchor for a selection.
fn nearest_char(chars: &[CharBox], x: f32, y: f32) -> usize {
    let mut best = 0;
    let mut best_distance = f32::MAX;

    for (index, CharBox { area, .. }) in chars.iter().enumerate() {
        let dx = (area.left - x).max(0.).max(x - area.right);
        let dy = (area.bottom - y).max(0.).max(y - area.top);
        let distance = dx * dx + dy * dy;
        if distance < best_distance {
            best_distance = distance;
            best = index;
            if distance == 0. {
                break;
            }
        }
    }

    best
}

/// Collapses a run of characters into one area per line of text.
fn group_into_lines(chars: &[CharBox]) -> Vec<Area> {
    let mut lines: Vec<Area> = Vec::new();

    for CharBox { area, .. } in chars {
        // Zero-width characters (spaces at line ends) would stretch a line's
        // box across the page.
        if area.width() <= 0. || area.height() <= 0. {
            continue;
        }

        match lines.last_mut() {
            Some(line) if line.shares_line_with(area) => *line = line.union(area),
            _ => lines.push(*area),
        }
    }

    lines
}

/// Paints marks into a rendered page, in place.
///
/// `pixels` is BGRA at `scale` device pixels per point; `page_height` is the
/// page height in points, needed to flip PDF space (origin bottom-left) into
/// image space (origin top-left).
fn blend_marks<'a>(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    page_height: f32,
    marks: impl Iterator<Item = &'a Mark>,
) {
    for mark in marks {
        let area = mark.painted_area();

        let left = (area.left * scale).round().max(0.) as u32;
        let right = (area.right * scale).round().min(width as f32) as u32;
        let top = ((page_height - area.top) * scale).round().max(0.) as u32;
        let bottom = ((page_height - area.bottom) * scale)
            .round()
            .min(height as f32) as u32;

        if right <= left || bottom <= top {
            continue;
        }

        let (red, green, blue) = (
            (mark.color >> 16) as u8,
            (mark.color >> 8) as u8,
            mark.color as u8,
        );
        let opacity = mark.opacity.clamp(0., 1.);

        for y in top..bottom {
            let row = (y * width) as usize * 4;
            for x in left..right {
                let pixel = row + x as usize * 4;
                let Some(bgra) = pixels.get_mut(pixel..pixel + 4) else {
                    continue;
                };

                match mark.kind {
                    // Multiply: paper takes the colour, glyphs stay dark.
                    // Opacity fades between the untouched pixel and the fully
                    // multiplied one.
                    MarkKind::Highlight => {
                        bgra[0] = mix(bgra[0], multiply(bgra[0], blue), opacity);
                        bgra[1] = mix(bgra[1], multiply(bgra[1], green), opacity);
                        bgra[2] = mix(bgra[2], multiply(bgra[2], red), opacity);
                    }
                    // A rule is drawn on top, like ink.
                    MarkKind::Underline => {
                        bgra[0] = mix(bgra[0], blue, opacity);
                        bgra[1] = mix(bgra[1], green, opacity);
                        bgra[2] = mix(bgra[2], red, opacity);
                    }
                }
            }
        }
    }
}

fn multiply(base: u8, tint: u8) -> u8 {
    ((base as u16 * tint as u16) / 255) as u8
}

/// `base` at `1 - amount`, `target` at `amount`.
fn mix(base: u8, target: u8, amount: f32) -> u8 {
    (base as f32 + (target as f32 - base as f32) * amount).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A visible character on a 10pt line whose baseline is at `bottom`.
    fn glyph(character: char, left: f32, bottom: f32) -> CharBox {
        CharBox {
            area: Area::new(left, bottom, left + 5., bottom + 10.),
            character,
        }
    }

    /// The degenerate box PDFium hands back for spaces and generated breaks.
    fn blank(character: char, left: f32, bottom: f32) -> CharBox {
        CharBox {
            area: Area::new(left, bottom, left, bottom),
            character,
        }
    }

    #[test]
    fn spaces_and_generated_breaks_do_not_split_lines() {
        let chars = [
            glyph('a', 0., 100.),
            glyph('b', 5., 100.),
            blank(' ', 10., 100.),
            glyph('c', 12., 100.),
            blank('\r', 17., 100.),
            blank('\n', 17., 100.),
            glyph('d', 0., 85.),
            blank(' ', 5., 85.),
            glyph('e', 7., 85.),
        ];

        assert_eq!(run_of_text(&chars), "ab c\nd e");
    }

    #[test]
    fn a_gap_with_no_space_character_still_reads_as_a_space() {
        let chars = [glyph('a', 0., 100.), glyph('b', 9., 100.)];
        assert_eq!(run_of_text(&chars), "a b");
    }
}
