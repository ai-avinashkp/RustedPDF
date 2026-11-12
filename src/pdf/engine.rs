//! Binding to the PDFium shared library.
//!
//! PDFium keeps its bindings in a process-wide slot, so there is exactly one
//! instance per process and it lives until exit — hence the leak, which makes
//! the borrow `'static` and lets documents be stored in view state.

use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use pdfium_render::prelude::*;

/// The leaked instance. PDFium refuses a second initialisation in the same
/// process (`Pdfium::new` asserts on it), so this has to be process-wide rather
/// than per-thread.
static INSTANCE: AtomicPtr<Pdfium> = AtomicPtr::new(std::ptr::null_mut());
static INITIALISING: Mutex<()> = Mutex::new(());

/// The PDFium instance, binding the library on first use.
///
/// The error is worth showing to the user verbatim: it names every path that
/// was tried, which is the only useful thing to say when the library is
/// missing.
///
/// Sharing one `&'static Pdfium` across threads is sound here because
/// `pdfium-render`'s `thread_safe` feature (on by default) serialises every
/// call into the library behind its own lock; the app itself only ever touches
/// PDFium from the UI thread.
pub fn pdfium() -> Result<&'static Pdfium> {
    if let Some(instance) = load() {
        return Ok(instance);
    }

    let _guard = INITIALISING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Another thread may have won the race while we waited.
    if let Some(instance) = load() {
        return Ok(instance);
    }

    let instance = Box::into_raw(Box::new(Pdfium::new(bind()?)));
    INSTANCE.store(instance, Ordering::Release);

    // SAFETY: leaked above and never freed, so the reference is `'static`.
    Ok(unsafe { &*instance })
}

fn load() -> Option<&'static Pdfium> {
    let instance = INSTANCE.load(Ordering::Acquire);
    // SAFETY: only ever set to a leaked, never-freed allocation.
    (!instance.is_null()).then(|| unsafe { &*instance })
}

/// Whether PDFium is available, without forcing the caller to handle an error.
pub fn is_available() -> bool {
    pdfium().is_ok()
}

fn bind() -> Result<Box<dyn PdfiumLibraryBindings>> {
    let mut attempts = Vec::new();

    for directory in search_paths() {
        let candidate = Pdfium::pdfium_platform_library_name_at_path(&directory);
        match Pdfium::bind_to_library(&candidate) {
            Ok(bindings) => return Ok(bindings),
            Err(error) => attempts.push(format!("  {} — {error}", candidate.display())),
        }
    }

    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(bindings),
        Err(error) => {
            attempts.push(format!("  system library path — {error}"));
            Err(anyhow!(
                "Could not load PDFium. Tried:\n{}\n\nSee vendor/pdfium/README.md.",
                attempts.join("\n")
            ))
        }
    }
}

/// Where to look for the library, in order: next to the executable (how a
/// built app ships), then the vendored copy relative to the working directory
/// (how it looks during development).
fn search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            paths.push(directory.to_path_buf());
        }
    }

    if let Ok(working_directory) = std::env::current_dir() {
        paths.push(working_directory.join("vendor").join("pdfium"));
        paths.push(working_directory);
    }

    paths
}
