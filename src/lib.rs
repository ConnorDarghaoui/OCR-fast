/// Orquestación del pipeline y estado de la interfaz interactiva.
pub mod application;
/// Reglas y tipos del dominio OCR independientes de infraestructura.
pub mod domain;
/// Adaptadores concretos para parsing, OCR, layout, exportación y persistencia.
pub mod infrastructure;
/// Puertos estables que separan casos de uso de implementaciones concretas.
pub mod interfaces;

/// Reexporta casos de uso y componentes de aplicación.
pub use application::*;
/// Reexporta el dominio para consumidores del crate.
pub use domain::*;
/// Reexporta implementaciones concretas listas para integración.
pub use infrastructure::*;
/// Reexporta los contratos públicos del sistema.
pub use interfaces::*;
