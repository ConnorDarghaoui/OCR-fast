use crate::domain::errors::LayoutError;
use crate::domain::Document;
use crate::infrastructure::page_composer::PageComposer;
use crate::interfaces::ports::DocumentAssemblerPort;

/// Adaptador histórico hacia el compositor canónico por página.
///
/// El ensamblador ya no define heurísticas propias. Se conserva para pruebas y
/// compatibilidad, pero delega en `PageComposer` para fijar el orden de lectura
/// in-place sobre el `Document`.
///
/// Este módulo no debe volver a convertirse en dueño de la política de orden.
/// Si la composición cambia, el cambio pertenece a `PageComposer`.
pub struct LayoutGuidedDocumentAssembler {
    composer: PageComposer,
}

impl LayoutGuidedDocumentAssembler {
    /// Construye un ensamblador sin estado mutable compartido.
    pub fn new() -> Self {
        Self {
            composer: PageComposer::new(),
        }
    }
}

impl Default for LayoutGuidedDocumentAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentAssemblerPort for LayoutGuidedDocumentAssembler {
    fn assemble(&self, document: &mut Document) -> Result<(), LayoutError> {
        self.composer.apply_document_order(document);
        document.metadata.insert(
            "assembly_strategy".to_string(),
            "page-composer-compat".to_string(),
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "LayoutGuidedDocumentAssembler"
    }
}
