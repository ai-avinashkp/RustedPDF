//! Puts the vendored PDFium shared library next to the built executable, so
//! `cargo run` and a copied-out binary both find it without any setup.
//!
//! See `vendor/pdfium/README.md` for where the library comes from.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=vendor/pdfium");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    embed_windows_resources();

    let library = if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("vendor").join("pdfium").join(library);

    if !source.exists() {
        println!(
            "cargo:warning=vendor/pdfium/{library} is missing — RustedPdf will start but can't \
             open PDFs. See vendor/pdfium/README.md."
        );
        return;
    }

    // OUT_DIR is target/<profile>/build/<pkg>-<hash>/out; the executable lands
    // three levels up.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(target_dir) = out_dir.ancestors().nth(3) else {
        return;
    };

    let destination = target_dir.join(library);
    if let Err(error) = fs::copy(&source, &destination) {
        // A running instance holds a lock on the DLL; that copy is already
        // current, so this isn't fatal.
        println!("cargo:warning=could not copy {library} to the target directory: {error}");
    }
}

/// Embeds the app icon and version metadata into the Windows executable, so
/// Explorer, the taskbar and the Alt+Tab switcher show the icon rather than the
/// generic one — and the file's Properties ▸ Details tab is filled in.
#[cfg(windows)]
fn embed_windows_resources() {
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    resource.set("ProductName", "RustedPdf");
    resource.set(
        "FileDescription",
        "RustedPdf — a PDF reader and highlighter",
    );
    resource.set("CompanyName", "RustedPdf");
    resource.set(
        "LegalCopyright",
        "MIT OR Apache-2.0. Includes PDFium (BSD-3-Clause).",
    );

    if let Err(error) = resource.compile() {
        // Worth building without: only the icon and metadata are lost.
        println!("cargo:warning=could not embed the Windows resources: {error}");
    }
}

#[cfg(not(windows))]
fn embed_windows_resources() {}
