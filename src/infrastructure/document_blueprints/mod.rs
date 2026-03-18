use crate::domain::errors::LayoutError;
use crate::domain::{
    AlignmentHint, Block, BlockType, Document, DocumentBlueprint, ElementBlueprint, ElementRole,
    EmphasisHint, ImageCropRef, Page, PageBlueprint, StyleHints,
};
use crate::interfaces::ports::DocumentBlueprintBuilderPort;

/// Reconstruye un blueprint visual estable para exportadores ricos.
///
/// El builder encapsula heurísticas de columnas, anclas y preservación de
/// activos visuales para que `DOCX`, `LaTeX` y futuros exportadores operen
/// sobre un modelo coherente en vez de reinterpretar bloques OCR ad hoc.
///
/// # Performance
///
/// Opera sobre referencias al `Document` ya materializado y evita duplicar bytes
/// raster. El coste es lineal respecto a bloques por página y no añade buffers
/// pesados más allá del resultado final serializable.
///
/// # Trade-offs
///
/// Las heurísticas están optimizadas para documentos escaneados de libros,
/// artículos y material fotocopiado. Layouts extremadamente libres pueden
/// requerir detectores semánticos adicionales o reglas específicas por dominio.
pub struct HighFidelityBlueprintBuilder;

impl HighFidelityBlueprintBuilder {
    /// Construye un builder sin estado compartido.
    pub fn new() -> Self {
        Self
    }

    fn construir_pagina(&self, pagina: &Page) -> Result<PageBlueprint, LayoutError> {
        if pagina.dimensions.width == 0 || pagina.dimensions.height == 0 {
            return Err(LayoutError::SegmentationError(format!(
                "pagina {} sin dimensiones validas para blueprint",
                pagina.number
            )));
        }

        let total_columnas = estimar_columnas(&pagina.blocks, pagina.dimensions.width);
        let bloques_ordenados =
            ordenar_bloques_para_lectura(&pagina.blocks, pagina.dimensions.width);
        let elementos = bloques_ordenados
            .into_iter()
            .enumerate()
            .map(|(indice, bloque)| {
                construir_elemento(pagina, bloque, indice as u32, total_columnas)
            })
            .collect();

        Ok(PageBlueprint {
            number: pagina.number,
            dimensions: pagina.dimensions.clone(),
            elements: elementos,
        })
    }
}

impl Default for HighFidelityBlueprintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBlueprintBuilderPort for HighFidelityBlueprintBuilder {
    fn build_blueprint(&self, document: &Document) -> Result<DocumentBlueprint, LayoutError> {
        let mut paginas = Vec::with_capacity(document.pages.len());
        for pagina in &document.pages {
            paginas.push(self.construir_pagina(pagina)?);
        }

        Ok(DocumentBlueprint {
            document_id: document.id.clone(),
            source_path: document.source_path.to_string_lossy().into_owned(),
            pages: paginas,
        })
    }

    fn name(&self) -> &str {
        "HighFidelityBlueprintBuilder"
    }
}

fn ordenar_bloques_para_lectura<'a>(bloques: &'a [Block], ancho_pagina: u32) -> Vec<&'a Block> {
    let mut bloques_base: Vec<&Block> = bloques.iter().collect();
    bloques_base.sort_by_key(|bloque| {
        (
            bloque.bounding_box.y,
            bloque.bounding_box.x,
            bloque.reading_order,
        )
    });

    if estimar_columnas(bloques, ancho_pagina) == 1 {
        return bloques_base;
    }

    let mut anclas = Vec::new();
    let mut resto = Vec::new();
    for bloque in bloques_base {
        if es_ancla_visual(bloque, ancho_pagina) {
            anclas.push(bloque);
        } else {
            resto.push(bloque);
        }
    }

    let mut orden = Vec::with_capacity(bloques.len());
    let mut inicio_segmento = 0;

    for ancla in anclas {
        let mut segmento = Vec::new();
        let mut pendientes = Vec::new();

        for bloque in resto {
            if bloque.bounding_box.y >= inicio_segmento
                && bloque.bounding_box.y < ancla.bounding_box.y
            {
                segmento.push(bloque);
            } else {
                pendientes.push(bloque);
            }
        }

        orden.extend(ordenar_segmento_en_columnas(segmento, ancho_pagina));
        orden.push(ancla);
        resto = pendientes;
        inicio_segmento = ancla
            .bounding_box
            .y
            .saturating_add(ancla.bounding_box.height);
    }

    orden.extend(ordenar_segmento_en_columnas(resto, ancho_pagina));
    orden
}

fn ordenar_segmento_en_columnas<'a>(bloques: Vec<&'a Block>, ancho_pagina: u32) -> Vec<&'a Block> {
    let mut izquierda = Vec::new();
    let mut derecha = Vec::new();
    let centro_pagina = ancho_pagina / 2;

    for bloque in bloques {
        if centro_x(bloque) <= centro_pagina {
            izquierda.push(bloque);
        } else {
            derecha.push(bloque);
        }
    }

    izquierda.sort_by_key(|bloque| (bloque.bounding_box.y, bloque.bounding_box.x));
    derecha.sort_by_key(|bloque| (bloque.bounding_box.y, bloque.bounding_box.x));
    izquierda.extend(derecha);
    izquierda
}

fn construir_elemento(
    pagina: &Page,
    bloque: &Block,
    orden_lectura: u32,
    total_columnas_pagina: u32,
) -> ElementBlueprint {
    let rol = mapear_rol(bloque.block_type);
    let usa_dos_columnas =
        total_columnas_pagina == 2 && !es_ancla_visual(bloque, pagina.dimensions.width);
    let total_columnas = if usa_dos_columnas { 2 } else { 1 };
    let indice_columna = if usa_dos_columnas && centro_x(bloque) > pagina.dimensions.width / 2 {
        1
    } else {
        0
    };

    ElementBlueprint {
        role: rol,
        bounding_box: bloque.bounding_box.clone(),
        reading_order: orden_lectura,
        column_index: indice_columna,
        total_columns: total_columnas,
        text: bloque.content.clone(),
        table: bloque.table_structure.clone(),
        image_crop: construir_referencia_imagen(pagina.number, bloque, rol),
        style: inferir_estilo(pagina, bloque, rol, total_columnas),
    }
}

fn construir_referencia_imagen(
    numero_pagina: u32,
    bloque: &Block,
    rol: ElementRole,
) -> Option<ImageCropRef> {
    if matches!(
        rol,
        ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp
    ) {
        return Some(ImageCropRef {
            page_number: numero_pagina,
            bounding_box: bloque.bounding_box.clone(),
        });
    }

    None
}

fn inferir_estilo(
    pagina: &Page,
    bloque: &Block,
    rol: ElementRole,
    total_columnas: u32,
) -> StyleHints {
    let ancho_pagina = pagina.dimensions.width.max(1);
    let alto_pagina = pagina.dimensions.height.max(1);
    let ratio_ancho = bloque.bounding_box.width as f32 / ancho_pagina as f32;
    let ratio_alto = bloque.bounding_box.height as f32 / alto_pagina as f32;
    let centro_pagina = ancho_pagina as f32 / 2.0;
    let centro_bloque = centro_x(bloque) as f32;
    let esta_centrado = (centro_bloque - centro_pagina).abs() <= (ancho_pagina as f32 * 0.12);

    let alineacion = if ratio_ancho >= 0.85 {
        AlignmentHint::FullWidth
    } else if matches!(rol, ElementRole::Title) && esta_centrado {
        AlignmentHint::Center
    } else if bloque.bounding_box.x >= ancho_pagina.saturating_mul(55) / 100 && ratio_ancho < 0.40 {
        AlignmentHint::Right
    } else {
        AlignmentHint::Left
    };

    let emphasis = match rol {
        ElementRole::Title => EmphasisHint::Strong,
        ElementRole::Separator | ElementRole::Stamp => EmphasisHint::Neutral,
        _ => EmphasisHint::Regular,
    };

    let font_scale = match rol {
        ElementRole::Title => (1.2 + ratio_alto * 14.0).clamp(1.4, 2.4),
        ElementRole::Paragraph | ElementRole::ListItem => (0.9 + ratio_alto * 6.0).clamp(0.9, 1.2),
        ElementRole::Table => 0.95,
        ElementRole::Formula => 1.1,
        _ => 1.0,
    };

    let preserve_positioning = matches!(
        rol,
        ElementRole::Table
            | ElementRole::Figure
            | ElementRole::Formula
            | ElementRole::Signature
            | ElementRole::Stamp
            | ElementRole::Separator
    ) || total_columnas > 1;

    StyleHints {
        alignment: alineacion,
        emphasis,
        font_scale,
        preserve_positioning,
    }
}

fn estimar_columnas(bloques: &[Block], ancho_pagina: u32) -> u32 {
    let ancho_pagina = ancho_pagina.max(1);
    let mut izquierda = 0usize;
    let mut derecha = 0usize;

    for bloque in bloques {
        if es_ancla_visual(bloque, ancho_pagina) {
            continue;
        }

        if !matches!(
            bloque.block_type,
            BlockType::Text | BlockType::List | BlockType::Table | BlockType::Formula
        ) {
            continue;
        }

        let centro = centro_x(bloque);
        if centro <= ancho_pagina / 2 {
            izquierda += 1;
        } else {
            derecha += 1;
        }
    }

    if izquierda >= 1 && derecha >= 1 {
        2
    } else {
        1
    }
}

fn es_ancla_visual(bloque: &Block, ancho_pagina: u32) -> bool {
    if matches!(bloque.block_type, BlockType::Title | BlockType::Separator) {
        return true;
    }

    let ratio_ancho = bloque.bounding_box.width as f32 / ancho_pagina.max(1) as f32;
    ratio_ancho >= 0.70
}

fn centro_x(bloque: &Block) -> u32 {
    bloque
        .bounding_box
        .x
        .saturating_add(bloque.bounding_box.width / 2)
}

fn mapear_rol(tipo: BlockType) -> ElementRole {
    match tipo {
        BlockType::Title => ElementRole::Title,
        BlockType::Text => ElementRole::Paragraph,
        BlockType::Table => ElementRole::Table,
        BlockType::Image => ElementRole::Figure,
        BlockType::Formula => ElementRole::Formula,
        BlockType::List => ElementRole::ListItem,
        BlockType::Signature => ElementRole::Signature,
        BlockType::Stamp => ElementRole::Stamp,
        BlockType::Separator => ElementRole::Separator,
        BlockType::Unknown => ElementRole::Unknown,
    }
}
