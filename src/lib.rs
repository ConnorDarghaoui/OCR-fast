//! OCRfast - Sistema OCR local-first implementado en Rust
//! 
//! Este crate proporciona funcionalidades para realizar reconocimiento óptico de caracteres
//! de manera local y eficiente, siguiendo principios de Clean Architecture.

pub mod domain;
pub mod application;
pub mod infrastructure;
pub mod interfaces;

// Re-exports convenientes para usuarios del crate
pub use domain::*;
pub use application::*;
pub use infrastructure::*;
pub use interfaces::*;