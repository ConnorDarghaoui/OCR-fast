/// Reconstrucción final del documento guiada por layout.
pub mod document_assemblers;
/// Builders de representación visual para exportación de alta fidelidad.
pub mod document_blueprints;
/// Adaptadores concretos de parsing y render de documentos.
pub mod document_parsers;
/// Materializadores de salida para TXT, LaTeX, PDF y JSON.
pub mod exporters;
/// Persistencia local de snapshots de trabajos.
pub mod job_store;
/// Motores de análisis geométrico de layout.
pub mod layout_engines;
/// Engines OCR concretos y sus backends.
pub mod ocr_engines;
/// Correcciones textuales posteriores a OCR.
pub mod postprocessors;
/// Transformaciones raster previas a inferencia.
pub mod preprocessors;

/// Reexporta ensambladores de documento concretos.
pub use document_assemblers::*;
/// Reexporta builders de blueprint listos para integración directa.
pub use document_blueprints::*;
/// Reexporta exportadores concretos para integración directa.
pub use exporters::*;
/// Reexporta stores concretos listos para uso local.
pub use job_store::*;
/// Reexporta motores de layout concretos.
pub use layout_engines::*;
/// Reexporta postprocesadores concretos.
pub use postprocessors::*;
/// Reexporta preprocesadores concretos.
pub use preprocessors::*;
