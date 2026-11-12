//! PDF back end: PDFium binding, document model, and the highlight/underline
//! marks layered on top of it.

pub mod document;
pub mod engine;
pub mod export;
pub mod mark;

pub use document::Document;
pub use export::ExportFormat;
pub use mark::{Area, Mark, MarkKind, Span};
