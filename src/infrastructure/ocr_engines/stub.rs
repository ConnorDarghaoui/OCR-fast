//! Stub OCR engine for testing without real dependencies.

use crate::domain::errors::OcrError;
use crate::domain::{Document, ProcessingProfile};
use crate::interfaces::ports::OcrEnginePort;

/// Stub OCR engine que genera texto simulado.
pub struct StubOcrEngine;

impl StubOcrEngine {
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

        // Simular procesamiento
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
