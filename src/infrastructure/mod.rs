//! Modulo de infraestructura: Implementaciones concretas de puertos y servicios externos.
//!
//! Este modulo contiene las implementaciones de bajo nivel que interactuan
//! con bibliotecas externas, sistemas de archivos, motores OCR, etc.
//! Todas las implementaciones cumplen con los contratos definidos en
//! el modulo `interfaces::ports`.

pub mod document_parsers;
pub mod exporters;
pub mod job_store;
pub mod layout_engines;
pub mod ocr_engines;
pub mod postprocessors;
pub mod preprocessors;

// Export specific items to avoid ambiguous_glob_reexports warning
pub use exporters::*;
pub use job_store::*;
pub use layout_engines::*;
pub use postprocessors::*;
pub use preprocessors::*;
