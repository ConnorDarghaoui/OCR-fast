//! Parsers de documentos: lectura de archivos PDF, imagenes, etc.

pub mod image_parser;
pub mod pdf_renderer;
pub mod stub;

// Re-export stub for convenience
pub use stub::StubDocumentParser;
