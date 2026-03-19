use crate::domain::errors::LayoutError;
use crate::domain::{Block, BlockType, Document, Page, ProcessingMode};
use crate::infrastructure::document_blueprints::{
    inferir_modos_procesamiento_por_pagina, persistir_modos_procesamiento_por_pagina,
};
use crate::interfaces::ports::DocumentAssemblerPort;

const UMBRAL_ANCHO_COMPLETO: f32 = 0.6;

/// Ensambla el documento final a partir de la estructura detectada por layout.
///
/// El ensamblador no vuelve a inferir semántica; solo traduce la geometría ya
/// detectada a una secuencia de lectura reproducible. La heurística actual trata
/// bloques de ancho completo como anclas de banda y ordena el resto por columnas,
/// con prioridad izquierda→derecha dentro de cada banda.
pub struct LayoutGuidedDocumentAssembler;

impl LayoutGuidedDocumentAssembler {
    /// Construye un ensamblador sin estado mutable compartido.
    pub fn new() -> Self {
        Self
    }

    fn reordenar_pagina(&self, pagina: &mut Page) {
        if pagina.blocks.len() <= 1 {
            Self::renumerar_orden_lectura(&mut pagina.blocks);
            return;
        }

        let mut bloques = std::mem::take(&mut pagina.blocks);
        bloques.sort_by(|a, b| {
            a.bounding_box
                .y
                .cmp(&b.bounding_box.y)
                .then(a.bounding_box.x.cmp(&b.bounding_box.x))
        });

        let ancho_pagina = pagina.dimensions.width.max(1) as f32;
        let mut bloques_ordenados = Vec::with_capacity(bloques.len());
        let mut seccion_columnar = Vec::new();

        for bloque in bloques {
            if Self::es_bloque_ancla(&bloque, ancho_pagina) {
                Self::vaciar_seccion_columnar(
                    &mut bloques_ordenados,
                    &mut seccion_columnar,
                    ancho_pagina,
                );
                bloques_ordenados.push(bloque);
            } else {
                seccion_columnar.push(bloque);
            }
        }

        Self::vaciar_seccion_columnar(&mut bloques_ordenados, &mut seccion_columnar, ancho_pagina);
        Self::renumerar_orden_lectura(&mut bloques_ordenados);
        pagina.blocks = bloques_ordenados;
    }

    fn preservar_orden_visual(&self, pagina: &mut Page) {
        pagina.blocks.sort_by(|a, b| {
            a.reading_order
                .cmp(&b.reading_order)
                .then(a.bounding_box.y.cmp(&b.bounding_box.y))
                .then(a.bounding_box.x.cmp(&b.bounding_box.x))
        });
        Self::renumerar_orden_lectura(&mut pagina.blocks);
    }

    fn es_bloque_ancla(bloque: &Block, ancho_pagina: f32) -> bool {
        let ancho_relativo = bloque.bounding_box.width as f32 / ancho_pagina;
        ancho_relativo >= UMBRAL_ANCHO_COMPLETO
            || matches!(bloque.block_type, BlockType::Title | BlockType::Separator)
    }

    fn vaciar_seccion_columnar(
        bloques_ordenados: &mut Vec<Block>,
        seccion_columnar: &mut Vec<Block>,
        ancho_pagina: f32,
    ) {
        if seccion_columnar.is_empty() {
            return;
        }

        if Self::usa_dos_columnas(seccion_columnar, ancho_pagina) {
            let mitad = ancho_pagina / 2.0;
            let mut columna_izquierda = Vec::new();
            let mut columna_derecha = Vec::new();

            for bloque in seccion_columnar.drain(..) {
                if Self::centro_x(&bloque) < mitad {
                    columna_izquierda.push(bloque);
                } else {
                    columna_derecha.push(bloque);
                }
            }

            Self::ordenar_top_down(&mut columna_izquierda);
            Self::ordenar_top_down(&mut columna_derecha);
            bloques_ordenados.extend(columna_izquierda);
            bloques_ordenados.extend(columna_derecha);
        } else {
            Self::ordenar_top_down(seccion_columnar);
            bloques_ordenados.extend(seccion_columnar.drain(..));
        }
    }

    fn usa_dos_columnas(bloques: &[Block], ancho_pagina: f32) -> bool {
        let mitad = ancho_pagina / 2.0;
        let mut izquierda = 0usize;
        let mut derecha = 0usize;
        let mut min_x_derecha = ancho_pagina as u32;
        let mut max_x_izquierda = 0u32;
        let mut top_izquierda = u32::MAX;
        let mut top_derecha = u32::MAX;

        for bloque in bloques {
            if Self::centro_x(bloque) < mitad {
                izquierda += 1;
                max_x_izquierda = max_x_izquierda.max(
                    bloque
                        .bounding_box
                        .x
                        .saturating_add(bloque.bounding_box.width),
                );
                top_izquierda = top_izquierda.min(bloque.bounding_box.y);
            } else {
                derecha += 1;
                min_x_derecha = min_x_derecha.min(bloque.bounding_box.x);
                top_derecha = top_derecha.min(bloque.bounding_box.y);
            }
        }

        if izquierda < 2 || derecha < 2 {
            return false;
        }

        let ancho_pagina = ancho_pagina.max(1.0) as u32;
        let gutter_minimo = ancho_pagina.saturating_mul(4) / 100;
        let gutter = min_x_derecha.saturating_sub(max_x_izquierda);
        let diferencia_top = top_izquierda.abs_diff(top_derecha);

        gutter >= gutter_minimo && diferencia_top <= 220
    }

    fn centro_x(bloque: &Block) -> f32 {
        bloque.bounding_box.x as f32 + bloque.bounding_box.width as f32 / 2.0
    }

    fn ordenar_top_down(bloques: &mut [Block]) {
        bloques.sort_by(|a, b| {
            a.bounding_box
                .y
                .cmp(&b.bounding_box.y)
                .then(a.bounding_box.x.cmp(&b.bounding_box.x))
        });
    }

    fn renumerar_orden_lectura(bloques: &mut [Block]) {
        for (indice, bloque) in bloques.iter_mut().enumerate() {
            bloque.reading_order = indice as u32;
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
        let page_processing_modes = inferir_modos_procesamiento_por_pagina(document);
        persistir_modos_procesamiento_por_pagina(document, &page_processing_modes);

        for (pagina, processing_mode) in document.pages.iter_mut().zip(page_processing_modes.iter())
        {
            match processing_mode {
                ProcessingMode::DocumentReconstruction => self.reordenar_pagina(pagina),
                ProcessingMode::VisualPreservation => self.preservar_orden_visual(pagina),
            }
        }

        document.metadata.insert(
            "assembly_strategy".to_string(),
            "layout-guided-page-policy".to_string(),
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "LayoutGuidedDocumentAssembler"
    }
}
