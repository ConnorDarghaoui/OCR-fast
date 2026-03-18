use crate::domain::errors::OcrError;
use crate::domain::{Document, ProcessingProfile};
use crate::interfaces::ports::OcrEnginePort;

/// Engine OCR stub para pruebas, demos y fallback operativo.
///
/// El stub produce contenido determinista y barato de generar. Su propósito no
/// es aproximar precisión OCR, sino permitir que TUI, pipeline y exportación
/// sigan siendo ejercitables cuando el backend real no está disponible.
pub struct StubOcrEngine;

impl StubOcrEngine {
    /// Construye un engine stub sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl OcrEnginePort for StubOcrEngine {
    fn process(
        &self,
        document: &mut Document,
        profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        log::info!("StubOcrEngine: procesando con perfil {:?}", profile);

        for page in &mut document.pages {
            for block in &mut page.blocks {
                block.content = match block.block_type {
                    crate::domain::BlockType::Title => "Documento de Prueba - Stub OCR".to_string(),
                    crate::domain::BlockType::Text => {
                        "Este es un texto de ejemplo generado por el StubOcrEngine. \
                         El contenido es simulado para testing de la interfaz TUI."
                            .to_string()
                    }
                    _ => format!("[{:?}] Contenido simulado", block.block_type),
                };

                block.confidence = match profile {
                    ProcessingProfile::Fast => 0.80,
                    ProcessingProfile::Balanced => 0.90,
                    ProcessingProfile::Accurate => 0.95,
                };
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "StubOcrEngine"
    }
}
