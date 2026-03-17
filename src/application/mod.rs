/// Orquestador del pipeline OCR y coordinación de casos de uso interactivos.
pub mod pipeline;
/// Interfaz terminal y estado reactivo de la aplicación.
pub mod tui;

/// Reexporta la TUI como punto de entrada principal de aplicación.
pub use tui::*;
