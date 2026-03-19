use crate::domain::errors::LayoutError;
use crate::domain::Document;
use crate::domain::DocumentBlueprint;
use crate::infrastructure::page_composer::{
    inferir_modos_procesamiento_por_pagina, persistir_modos_procesamiento_por_pagina, PageComposer,
};
use crate::interfaces::ports::DocumentBlueprintBuilderPort;

pub use crate::infrastructure::page_composer::{
    inferir_modos_procesamiento_por_pagina as infer_page_processing_modes,
    persistir_modos_procesamiento_por_pagina as persist_page_processing_modes,
};

/// Adaptador de compatibilidad hacia la composición canónica por página.
///
/// La lógica de reconstrucción visual ya no vive aquí. `HighFidelityBlueprintBuilder`
/// se conserva como fachada estable para pruebas y llamadas existentes, pero
/// delega completamente en `PageComposer`.
///
/// No se deben añadir heurísticas nuevas a este módulo. Si aparece una nueva
/// regla de composición, debe implementarse en `PageComposer` y este adaptador
/// debe seguir siendo fino.
pub struct HighFidelityBlueprintBuilder {
    composer: PageComposer,
}

impl HighFidelityBlueprintBuilder {
    /// Construye un builder sin estado compartido.
    pub fn new() -> Self {
        Self {
            composer: PageComposer::new(),
        }
    }

    /// API explícita de composición usada por el adaptador.
    pub fn compose(&self, document: &Document) -> Result<DocumentBlueprint, LayoutError> {
        self.composer.compose(document)
    }

    /// Reordena el documento in-place usando la política del compositor.
    pub fn reorder_document(&self, document: &mut Document) {
        self.composer.apply_document_order(document);
    }
}

impl Default for HighFidelityBlueprintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBlueprintBuilderPort for HighFidelityBlueprintBuilder {
    fn build_blueprint(&self, document: &Document) -> Result<DocumentBlueprint, LayoutError> {
        self.compose(document)
    }

    fn name(&self) -> &str {
        "HighFidelityBlueprintBuilder"
    }
}

#[allow(dead_code)]
pub(crate) fn _compat_reorder(document: &mut Document) {
    let modos = inferir_modos_procesamiento_por_pagina(document);
    persistir_modos_procesamiento_por_pagina(document, &modos);
}
