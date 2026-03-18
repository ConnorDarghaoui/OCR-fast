/// Parser coordinador para estrategias de imagen raster, TIFF y PDF.
pub mod image_parser;
/// Renderizador PDF respaldado por Pdfium.
pub mod pdf_renderer;
mod pdf_strategy;
mod raster_strategy;
/// Implementación stub para pruebas y modo degradado.
pub mod stub;
mod tiff_strategy;

/// Reexporta el parser stub para pruebas y demos.
pub use stub::StubDocumentParser;
