//! Ejemplo de uso del sistema OCRfast.
//!
//! Este ejemplo demuestra como configurar y usar el sistema OCR
//! con inyeccion de dependencias.

use ocrfast::domain::{LanguageConfig, ProcessingProfile};

fn main() {
    println!("Ejemplo de uso del sistema OCRfast");
    println!("===================================\n");

    // Mostrar perfiles disponibles
    println!("Perfiles de procesamiento disponibles:");
    println!("  - ProcessingProfile::Fast     (prioriza velocidad)");
    println!("  - ProcessingProfile::Accurate (prioriza precision)");
    println!("  - ProcessingProfile::Balanced (equilibrado)\n");

    // Mostrar configuracion de idioma por defecto
    let lang_config = LanguageConfig::default();
    println!("Configuracion de idioma por defecto:");
    println!("  - Idioma principal: {}", lang_config.primary);
    println!("  - Idiomas secundarios: {:?}\n", lang_config.secondary);

    // Mostrar perfil por defecto
    let profile = ProcessingProfile::default();
    println!("Perfil por defecto: {:?}\n", profile);

    println!("Para usar el sistema desde linea de comandos:");
    println!("  ocrfast --input documento.pdf --output resultado.md --profile accurate");
}
