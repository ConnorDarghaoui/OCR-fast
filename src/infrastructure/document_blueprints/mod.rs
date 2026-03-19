use crate::domain::errors::LayoutError;
use crate::domain::{
    AlignmentHint, Block, BlockType, Document, DocumentBlueprint, ElementBlueprint, ElementRole,
    EmphasisHint, ImageCropRef, Page, PageBlueprint, ProcessingMode, StyleHints,
    DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE,
};
use crate::interfaces::ports::DocumentBlueprintBuilderPort;

/// Reconstruye un blueprint visual estable para exportadores ricos.
///
/// El builder encapsula heurísticas de columnas, anclas y preservación de
/// activos visuales para que `LaTeX`, PDF y futuros exportadores operen
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

    fn construir_pagina(
        &self,
        pagina: &Page,
        processing_mode: ProcessingMode,
    ) -> Result<PageBlueprint, LayoutError> {
        if pagina.dimensions.width == 0 || pagina.dimensions.height == 0 {
            return Err(LayoutError::SegmentationError(format!(
                "pagina {} sin dimensiones validas para blueprint",
                pagina.number
            )));
        }

        let total_columnas = match processing_mode {
            ProcessingMode::DocumentReconstruction => {
                estimar_columnas(&pagina.blocks, pagina.dimensions.width)
            }
            ProcessingMode::VisualPreservation => 1,
        };
        let bloques_ordenados = match processing_mode {
            ProcessingMode::DocumentReconstruction => {
                ordenar_bloques_para_lectura(&pagina.blocks, pagina.dimensions.width)
            }
            ProcessingMode::VisualPreservation => ordenar_bloques_modo_visual(&pagina.blocks),
        };
        let bases_columna = inferir_bases_columna(&bloques_ordenados, pagina.dimensions.width);
        let mut elementos = Vec::with_capacity(bloques_ordenados.len());

        for (indice, bloque) in bloques_ordenados.iter().enumerate() {
            let orden_lectura = indice as u32;
            let rol = mapear_rol(bloque.block_type);
            let usa_dos_columnas = processing_mode == ProcessingMode::DocumentReconstruction
                && total_columnas == 2
                && !es_ancla_visual(bloque, pagina.dimensions.width);
            let columnas_elemento = if usa_dos_columnas { 2 } else { 1 };
            let indice_columna =
                if usa_dos_columnas && centro_x(bloque) > pagina.dimensions.width / 2 {
                    1
                } else {
                    0
                };
            let spacing_before_pt = inferir_espaciado_previo(
                &bloques_ordenados,
                indice,
                pagina.dimensions.width,
                columnas_elemento,
                indice_columna,
            );
            let left_indent_pt =
                inferir_indentacion_pt(*bloque, indice_columna, columnas_elemento, &bases_columna);

            elementos.push(construir_elemento(
                pagina,
                bloque,
                rol,
                orden_lectura,
                indice_columna,
                columnas_elemento,
                processing_mode,
                spacing_before_pt,
                left_indent_pt,
            ));
        }

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
        let processing_mode = inferir_processing_mode(document);
        let mut paginas = Vec::with_capacity(document.pages.len());
        for pagina in &document.pages {
            paginas.push(self.construir_pagina(pagina, processing_mode)?);
        }
        if processing_mode == ProcessingMode::DocumentReconstruction {
            marcar_hints_encabezado_pie(&mut paginas);
        }

        Ok(DocumentBlueprint {
            document_id: document.id.clone(),
            source_path: document.source_path.to_string_lossy().into_owned(),
            processing_mode,
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

fn ordenar_bloques_modo_visual<'a>(bloques: &'a [Block]) -> Vec<&'a Block> {
    let mut bloques_base: Vec<&Block> = bloques.iter().collect();
    bloques_base.sort_by_key(|bloque| {
        (
            bloque.reading_order,
            bloque.bounding_box.y,
            bloque.bounding_box.x,
        )
    });
    bloques_base
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
    rol: ElementRole,
    orden_lectura: u32,
    indice_columna: u32,
    total_columnas: u32,
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
) -> ElementBlueprint {
    ElementBlueprint {
        role: rol,
        bounding_box: bloque.bounding_box.clone(),
        reading_order: orden_lectura,
        column_index: indice_columna,
        total_columns: total_columnas,
        text: bloque.content.clone(),
        ocr_confidence: Some(bloque.confidence as f32),
        layout_confidence: bloque.layout_confidence.map(|valor| valor as f32),
        suspected_header: false,
        suspected_footer: false,
        table: bloque.table_structure.clone(),
        image_crop: construir_referencia_imagen(pagina.number, bloque, rol),
        style: inferir_estilo(
            pagina,
            bloque,
            rol,
            total_columnas,
            processing_mode,
            spacing_before_pt,
            left_indent_pt,
        ),
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
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
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

    let preserve_positioning = processing_mode == ProcessingMode::VisualPreservation
        || matches!(
            rol,
            ElementRole::Table
                | ElementRole::Figure
                | ElementRole::Formula
                | ElementRole::Signature
                | ElementRole::Stamp
                | ElementRole::Separator
        )
        || total_columnas > 1;

    StyleHints {
        alignment: alineacion,
        emphasis,
        font_scale,
        spacing_before_pt,
        left_indent_pt,
        keep_with_next: matches!(rol, ElementRole::Title | ElementRole::Separator),
        preserve_positioning,
    }
}

fn inferir_processing_mode(document: &Document) -> ProcessingMode {
    if let Some(forzado) = processing_mode_forzado(document) {
        return forzado;
    }

    if document.pages.is_empty() {
        return ProcessingMode::DocumentReconstruction;
    }

    let fuente_raster = document
        .source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp"
            )
        });

    let paginas_visuales = document
        .pages
        .iter()
        .filter(|pagina| parece_pagina_visual(pagina, fuente_raster))
        .count();

    if paginas_visuales > 0 && paginas_visuales * 2 >= document.pages.len() {
        ProcessingMode::VisualPreservation
    } else {
        ProcessingMode::DocumentReconstruction
    }
}

fn processing_mode_forzado(document: &Document) -> Option<ProcessingMode> {
    let valor = document
        .metadata
        .get(DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE)?;

    match valor.as_str() {
        "document" => Some(ProcessingMode::DocumentReconstruction),
        "visual" => Some(ProcessingMode::VisualPreservation),
        "auto" => None,
        _ => None,
    }
}

fn parece_pagina_visual(pagina: &Page, fuente_raster: bool) -> bool {
    let area_pagina =
        (pagina.dimensions.width.max(1) as f64) * (pagina.dimensions.height.max(1) as f64);
    let mut area_imagen_total = 0.0f64;
    let mut area_imagen_maxima = 0.0f64;
    let mut area_texto_total = 0.0f64;
    let mut bloques_textuales = 0usize;
    let mut bloques_textuales_anchos = 0usize;
    let mut bloques_titulo = 0usize;
    let mut bloques_tabla = 0usize;

    for bloque in &pagina.blocks {
        let area_bloque = (bloque.bounding_box.width as f64) * (bloque.bounding_box.height as f64);
        let ratio_ancho = bloque.bounding_box.width as f64 / pagina.dimensions.width.max(1) as f64;

        match bloque.block_type {
            BlockType::Image => {
                area_imagen_total += area_bloque;
                area_imagen_maxima = area_imagen_maxima.max(area_bloque);
            }
            BlockType::Text | BlockType::List | BlockType::Formula => {
                area_texto_total += area_bloque;
                bloques_textuales += 1;
                if ratio_ancho >= 0.55 {
                    bloques_textuales_anchos += 1;
                }
            }
            BlockType::Title => {
                area_texto_total += area_bloque;
                bloques_textuales += 1;
                bloques_titulo += 1;
                if ratio_ancho >= 0.55 {
                    bloques_textuales_anchos += 1;
                }
            }
            BlockType::Table => {
                area_texto_total += area_bloque;
                bloques_textuales += 1;
                bloques_tabla += 1;
                if ratio_ancho >= 0.55 {
                    bloques_textuales_anchos += 1;
                }
            }
            _ => {}
        }
    }

    let ratio_imagen_total = area_imagen_total / area_pagina;
    let ratio_imagen_maxima = area_imagen_maxima / area_pagina;
    let ratio_texto_total = area_texto_total / area_pagina;

    let tiene_imagen_destacada = ratio_imagen_maxima >= 0.12 || ratio_imagen_total >= 0.18;
    let sin_semantica_documental = bloques_titulo == 0 && bloques_tabla == 0;
    let texto_fragmentado = bloques_textuales >= 2 && bloques_textuales_anchos <= 1;
    let poca_cobertura_textual = ratio_texto_total <= 0.33;
    let parece_ui_fragmentada =
        fuente_raster && bloques_textuales >= 3 && bloques_textuales_anchos == 0;

    fuente_raster
        && ((tiene_imagen_destacada
            && sin_semantica_documental
            && (texto_fragmentado || poca_cobertura_textual))
            || (parece_ui_fragmentada && poca_cobertura_textual))
}

fn inferir_bases_columna(bloques: &[&Block], ancho_pagina: u32) -> [u32; 2] {
    let mut base_izquierda = ancho_pagina;
    let mut base_derecha = ancho_pagina;

    for bloque in bloques {
        if es_ancla_visual(bloque, ancho_pagina) {
            continue;
        }

        if centro_x(bloque) <= ancho_pagina / 2 {
            base_izquierda = base_izquierda.min(bloque.bounding_box.x);
        } else {
            base_derecha = base_derecha.min(bloque.bounding_box.x);
        }
    }

    if base_izquierda == ancho_pagina {
        base_izquierda = 0;
    }
    if base_derecha == ancho_pagina {
        base_derecha = ancho_pagina / 2;
    }

    [base_izquierda, base_derecha]
}

fn inferir_espaciado_previo(
    bloques: &[&Block],
    indice_actual: usize,
    ancho_pagina: u32,
    total_columnas: u32,
    indice_columna: u32,
) -> f32 {
    if indice_actual == 0 {
        return 0.0;
    }

    let actual = bloques[indice_actual];

    for previo in bloques[..indice_actual].iter().rev() {
        let columnas_previas = if es_ancla_visual(previo, ancho_pagina) {
            1
        } else {
            total_columnas
        };
        let columna_previa = indice_columna_bloque(previo, columnas_previas, ancho_pagina);

        if columnas_previas != total_columnas || columna_previa != indice_columna {
            continue;
        }

        let fin_previo = previo
            .bounding_box
            .y
            .saturating_add(previo.bounding_box.height);
        let gap_px = actual.bounding_box.y.saturating_sub(fin_previo);
        return px_a_pt_local(gap_px).clamp(0.0, 32.0) as f32;
    }

    0.0
}

fn indice_columna_bloque(bloque: &Block, total_columnas: u32, ancho_pagina: u32) -> u32 {
    if total_columnas == 2 && centro_x(bloque) > ancho_pagina / 2 {
        1
    } else {
        0
    }
}

fn inferir_indentacion_pt(
    bloque: &Block,
    indice_columna: u32,
    total_columnas: u32,
    bases_columna: &[u32; 2],
) -> f32 {
    if total_columnas == 1 {
        return 0.0;
    }

    let base_x = bases_columna[indice_columna as usize];
    px_a_pt_local(bloque.bounding_box.x.saturating_sub(base_x)) as f32
}

fn px_a_pt_local(px: u32) -> f64 {
    (px as f64) * (72.0 / 150.0)
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

fn marcar_hints_encabezado_pie(paginas: &mut [PageBlueprint]) {
    if paginas.len() < 2 {
        return;
    }

    for indice_pagina in 0..paginas.len() {
        let (paginas_previas, pagina_actual_y_resto) = paginas.split_at_mut(indice_pagina);
        let Some((pagina_actual, paginas_siguientes)) = pagina_actual_y_resto.split_first_mut()
        else {
            continue;
        };

        let pagina_previa = paginas_previas.last();
        let pagina_siguiente = paginas_siguientes.first();
        let altura = pagina_actual.dimensions.height.max(1);

        for indice_elemento in 0..pagina_actual.elements.len() {
            let elemento = &pagina_actual.elements[indice_elemento];
            let es_header = es_candidato_header_footer(elemento, altura, true)
                && (pagina_previa
                    .is_some_and(|vecina| tiene_match_repetido(elemento, vecina, true))
                    || pagina_siguiente
                        .is_some_and(|vecina| tiene_match_repetido(elemento, vecina, true)));

            let es_footer = es_candidato_header_footer(elemento, altura, false)
                && (pagina_previa
                    .is_some_and(|vecina| tiene_match_repetido(elemento, vecina, false))
                    || pagina_siguiente
                        .is_some_and(|vecina| tiene_match_repetido(elemento, vecina, false)));

            let elemento_mut = &mut pagina_actual.elements[indice_elemento];
            elemento_mut.suspected_header = es_header;
            elemento_mut.suspected_footer = es_footer;
        }
    }
}

fn es_candidato_header_footer(
    elemento: &ElementBlueprint,
    altura_pagina: u32,
    es_header: bool,
) -> bool {
    if !matches!(
        elemento.role,
        ElementRole::Title
            | ElementRole::Paragraph
            | ElementRole::ListItem
            | ElementRole::Separator
    ) {
        return false;
    }

    let texto_normalizado = normalizar_texto_repetido(&elemento.text);
    if texto_normalizado.is_empty() {
        return false;
    }

    let top = elemento.bounding_box.y;
    let bottom = elemento
        .bounding_box
        .y
        .saturating_add(elemento.bounding_box.height);

    if es_header {
        top <= altura_pagina.saturating_mul(14) / 100
    } else {
        bottom >= altura_pagina.saturating_mul(88) / 100
    }
}

fn tiene_match_repetido(
    candidato: &ElementBlueprint,
    vecina: &PageBlueprint,
    es_header: bool,
) -> bool {
    let altura_vecina = vecina.dimensions.height.max(1);
    vecina.elements.iter().any(|otro| {
        es_candidato_header_footer(otro, altura_vecina, es_header)
            && mismo_patron_repetido(candidato, otro, vecina.dimensions.width.max(1))
    })
}

fn mismo_patron_repetido(
    left: &ElementBlueprint,
    right: &ElementBlueprint,
    ancho_pagina: u32,
) -> bool {
    if left.role != right.role
        || left.column_index != right.column_index
        || left.total_columns != right.total_columns
    {
        return false;
    }

    let tolerancia_x = ancho_pagina.saturating_mul(5) / 100;
    let tolerancia_w = ancho_pagina.saturating_mul(8) / 100;
    let tolerancia_y = 36;

    if left.bounding_box.x.abs_diff(right.bounding_box.x) > tolerancia_x
        || left.bounding_box.width.abs_diff(right.bounding_box.width) > tolerancia_w
        || left.bounding_box.y.abs_diff(right.bounding_box.y) > tolerancia_y
    {
        return false;
    }

    let left_text = normalizar_texto_repetido(&left.text);
    let right_text = normalizar_texto_repetido(&right.text);
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }

    if left_text == right_text {
        return true;
    }

    similitud_textual(&left_text, &right_text) >= 0.86
}

fn normalizar_texto_repetido(texto: &str) -> String {
    let mut normalizado = String::with_capacity(texto.len());
    let mut previo_espacio = false;

    for caracter in texto.chars().flat_map(char::to_lowercase) {
        let emitido = if caracter.is_ascii_digit() {
            Some('#')
        } else if caracter.is_alphanumeric() {
            Some(caracter)
        } else if caracter.is_whitespace() || matches!(caracter, '-' | '–' | '—' | '_' | '/' | ':')
        {
            Some(' ')
        } else {
            None
        };

        match emitido {
            Some(' ') if !previo_espacio => {
                normalizado.push(' ');
                previo_espacio = true;
            }
            Some(' ') => {}
            Some(valor) => {
                normalizado.push(valor);
                previo_espacio = false;
            }
            None => {}
        }
    }

    normalizado.trim().to_string()
}

fn similitud_textual(left: &str, right: &str) -> f32 {
    let left_tokens: Vec<&str> = left.split_whitespace().collect();
    let right_tokens: Vec<&str> = right.split_whitespace().collect();
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let matches = left_tokens
        .iter()
        .filter(|token| right_tokens.contains(token))
        .count();

    let max_tokens = left_tokens.len().max(right_tokens.len()) as f32;
    matches as f32 / max_tokens
}
