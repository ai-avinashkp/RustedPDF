//! Exporting a chosen set of pages as images or as a new PDF.
//!
//! Both formats carry the marks: the PNG path renders exactly what the viewer
//! shows, and the PDF path materialises the marks as annotations before copying
//! the pages, then strips them again — the same trick [`super::Document::save`]
//! uses to keep the in-memory document clean.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _, Result};
use image::{ImageFormat, RgbaImage};
use zip::write::SimpleFileOptions;

/// How many device pixels per PDF point exported images are rendered at.
/// 2.0 ≈ 144 dpi, which stays sharp when the image is viewed full size.
pub const EXPORT_SCALE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// One PNG per page; several pages are bundled into a `.zip`.
    Png,
    /// A new PDF holding just the chosen pages.
    Pdf,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Png => "PNG image",
            ExportFormat::Pdf => "PDF document",
        }
    }

    /// The extension the save dialog should offer for `page_count` pages.
    pub fn extension(self, page_count: usize) -> &'static str {
        match self {
            ExportFormat::Png if page_count > 1 => "zip",
            ExportFormat::Png => "png",
            ExportFormat::Pdf => "pdf",
        }
    }

    pub fn describe(self, page_count: usize) -> String {
        match (self, page_count) {
            (ExportFormat::Png, 1) => "one PNG image".to_string(),
            (ExportFormat::Png, count) => format!("{count} PNG images in a .zip"),
            (ExportFormat::Pdf, 1) => "a one-page PDF".to_string(),
            (ExportFormat::Pdf, count) => format!("a {count}-page PDF"),
        }
    }
}

/// A page rasterised for export: BGRA bytes, as they come off PDFium.
pub struct RenderedPage {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Writes one page as a PNG, or several as a `.zip` of PNGs.
///
/// Pages are rendered one at a time, through `render`, and each is encoded and
/// written before the next is asked for. Holding them all first is what a
/// straightforward version would do, and it is exactly what makes "export every
/// page" of a book fall over: an A4 page at [`EXPORT_SCALE`] is 8 MB of BGRA,
/// so five hundred of them is four gigabytes. This way the peak is one page.
pub fn write_images(
    pages: &[usize],
    destination: &Path,
    mut render: impl FnMut(usize) -> Option<RenderedPage>,
) -> Result<()> {
    let &first = pages
        .first()
        .ok_or_else(|| anyhow!("No pages were selected."))?;

    if pages.len() == 1 {
        let page =
            render(first).ok_or_else(|| anyhow!("Page {} could not be rendered.", first + 1))?;
        let png = encode_png(&page)?;
        std::fs::write(destination, png)
            .with_context(|| format!("Could not write {}", destination.display()))?;
        return Ok(());
    }

    let file = std::fs::File::create(destination)
        .with_context(|| format!("Could not write {}", destination.display()))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let mut written = 0;
    for index in pages.iter().copied() {
        // A page PDFium refuses is skipped rather than losing the whole
        // export; the count below is what reports it.
        let Some(page) = render(index) else { continue };
        archive.start_file(format!("page-{:03}.png", page.index + 1), options)?;
        archive.write_all(&encode_png(&page)?)?;
        written += 1;
    }
    archive.finish()?;

    if written == 0 {
        return Err(anyhow!("None of the chosen pages could be rendered."));
    }

    Ok(())
}

fn encode_png(page: &RenderedPage) -> Result<Vec<u8>> {
    // PDFium hands back BGRA; PNG wants RGBA.
    let mut rgba = page.bgra.clone();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let image = RgbaImage::from_raw(page.width, page.height, rgba)
        .ok_or_else(|| anyhow!("Page {} could not be encoded.", page.index + 1))?;

    let mut encoded = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
        .with_context(|| format!("Could not encode page {}", page.index + 1))?;
    Ok(encoded)
}

/// Adds the right extension if the user didn't type one.
pub fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    match path.extension() {
        Some(existing) if existing.to_string_lossy().eq_ignore_ascii_case(extension) => path,
        _ => path.with_extension(extension),
    }
}

/// A page range string in PDFium's format ("1,3,5-9"), from zero-based indices.
pub fn page_range_string(pages: &[usize]) -> String {
    pages
        .iter()
        .map(|index| (index + 1).to_string())
        .collect::<Vec<_>>()
        .join(",")
}
