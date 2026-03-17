/// Parser híbrido para imágenes raster y documentos PDF.
pub mod image_parser;
/// Renderizador PDF respaldado por Pdfium.
pub mod pdf_renderer;
/// Implementación stub para pruebas y modo degradado.
pub mod stub;

/// Reexporta el parser stub para pruebas y demos.
pub use stub::StubDocumentParser;
