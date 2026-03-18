use crate::domain::errors::LayoutError;
use crate::domain::{Block, BlockType, Document, Page};
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

        for bloque in bloques {
            if Self::centro_x(bloque) < mitad {
                izquierda += 1;
            } else {
                derecha += 1;
            }
        }

        izquierda > 0 && derecha > 0
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
        for pagina in &mut document.pages {
            self.reordenar_pagina(pagina);
        }

        document
            .metadata
            .insert("assembly_strategy".to_string(), "layout-guided".to_string());
        Ok(())
    }

    fn name(&self) -> &str {
        "LayoutGuidedDocumentAssembler"
    }
}
