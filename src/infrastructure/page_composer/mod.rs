use crate::domain::errors::LayoutError;
use crate::domain::{
    AlignmentHint, Block, BlockContent, BlockType, Document, DocumentBlueprint, ElementBlueprint,
    ElementRole, EmphasisHint, Page, PageBlueprint, ProcessingMode, ResolvedBlock, StyleHints,
    DOCUMENT_METADATA_PAGE_PROCESSING_MODES, DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE,
};
use crate::infrastructure::automata::BlockAutomata;

/// Capa única de composición por página para renderizadores ricos.
///
/// `PageComposer` reemplaza la duplicación entre ensamblador y blueprint
/// builder. Decide el modo efectivo por página, fija el orden de lectura,
/// resuelve bloques mediante el autómata y proyecta una representación visual
/// única consumible por PDF, LaTeX, TXT y JSON.
pub struct PageComposer {
    automata: BlockAutomata,
}

impl PageComposer {
    /// Construye un compositor sin estado compartido.
    pub fn new() -> Self {
        Self {
            automata: BlockAutomata::new(),
        }
    }

    /// Compone el documento completo a una representación visual estable.
    pub fn compose(&self, document: &Document) -> Result<DocumentBlueprint, LayoutError> {
        let page_processing_modes = inferir_modos_procesamiento_por_pagina(document);
        let processing_mode = resumir_processing_mode(&page_processing_modes);
        let mut pages = Vec::with_capacity(document.pages.len());

        for (page, page_processing_mode) in document.pages.iter().zip(page_processing_modes.iter())
        {
            pages.push(self.compose_page(page, *page_processing_mode)?);
        }

        if pages
            .iter()
            .any(|page| page.processing_mode == ProcessingMode::DocumentReconstruction)
        {
            marcar_hints_encabezado_pie(&mut pages);
        }

        Ok(DocumentBlueprint {
            document_id: document.id.clone(),
            source_path: document.source_path.to_string_lossy().into_owned(),
            processing_mode,
            pages,
        })
    }

    /// Reordena el documento in-place usando la misma política del compositor.
    ///
    /// Esta API existe para mantener compatibilidad con el ensamblador histórico
    /// sin volver a duplicar heurísticas en otro módulo.
    pub fn apply_document_order(&self, document: &mut Document) {
        let page_processing_modes = inferir_modos_procesamiento_por_pagina(document);
        persistir_modos_procesamiento_por_pagina(document, &page_processing_modes);

        for (page, processing_mode) in document.pages.iter_mut().zip(page_processing_modes.iter()) {
            match processing_mode {
                ProcessingMode::DocumentReconstruction => reorder_page_document(page),
                ProcessingMode::VisualPreservation => preserve_page_visual_order(page),
            }
        }
    }

    fn compose_page(
        &self,
        page: &Page,
        page_processing_mode: ProcessingMode,
    ) -> Result<PageBlueprint, LayoutError> {
        if page.dimensions.width == 0 || page.dimensions.height == 0 {
            return Err(LayoutError::SegmentationError(format!(
                "pagina {} sin dimensiones validas para composicion",
                page.number
            )));
        }

        let ordered_blocks = match page_processing_mode {
            ProcessingMode::DocumentReconstruction => {
                order_blocks_for_document(&page.blocks, page.dimensions.width)
            }
            ProcessingMode::VisualPreservation => order_blocks_for_visual(&page.blocks),
        };

        let total_columns = match page_processing_mode {
            ProcessingMode::DocumentReconstruction => {
                estimate_columns(&page.blocks, page.dimensions.width)
            }
            ProcessingMode::VisualPreservation => 1,
        };
        let column_bases = infer_column_bases(&ordered_blocks, page.dimensions.width);
        let mut elements = Vec::with_capacity(ordered_blocks.len());

        for (index, block) in ordered_blocks.iter().enumerate() {
            let resolved = self.automata.resolve_block(page, block);
            let use_two_columns = page_processing_mode == ProcessingMode::DocumentReconstruction
                && total_columns == 2
                && !is_visual_anchor(block, page.dimensions.width);
            let element_columns = if use_two_columns { 2 } else { 1 };
            let column_index = if use_two_columns && center_x(block) > page.dimensions.width / 2 {
                1
            } else {
                0
            };
            let spacing_before_pt = infer_spacing_before(
                &ordered_blocks,
                index,
                page.dimensions.width,
                element_columns,
                column_index,
            );
            let left_indent_pt =
                infer_indent_pt(block, column_index, element_columns, &column_bases);

            elements.push(build_element(
                page,
                &resolved,
                index as u32,
                column_index,
                element_columns,
                page_processing_mode,
                spacing_before_pt,
                left_indent_pt,
            ));
        }

        Ok(PageBlueprint {
            number: page.number,
            dimensions: page.dimensions.clone(),
            processing_mode: page_processing_mode,
            elements,
        })
    }
}

impl Default for PageComposer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn inferir_modos_procesamiento_por_pagina(document: &Document) -> Vec<ProcessingMode> {
    if document.pages.is_empty() {
        return Vec::new();
    }

    if let Some(forzado) = processing_mode_forzado(document) {
        return vec![forzado; document.pages.len()];
    }

    if let Some(persistidos) = processing_modes_persistidos(document) {
        return persistidos;
    }

    document
        .pages
        .iter()
        .map(infer_processing_mode_for_page)
        .collect()
}

pub fn persistir_modos_procesamiento_por_pagina(
    document: &mut Document,
    page_processing_modes: &[ProcessingMode],
) {
    if page_processing_modes.len() != document.pages.len() {
        return;
    }

    let value = page_processing_modes
        .iter()
        .map(serialize_processing_mode)
        .collect::<Vec<_>>()
        .join(",");
    document
        .metadata
        .insert(DOCUMENT_METADATA_PAGE_PROCESSING_MODES.to_string(), value);
}

fn processing_mode_forzado(document: &Document) -> Option<ProcessingMode> {
    let value = document
        .metadata
        .get(DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE)?;

    match value.as_str() {
        "document" => Some(ProcessingMode::DocumentReconstruction),
        "visual" => Some(ProcessingMode::VisualPreservation),
        "auto" => None,
        _ => None,
    }
}

fn processing_modes_persistidos(document: &Document) -> Option<Vec<ProcessingMode>> {
    let value = document
        .metadata
        .get(DOCUMENT_METADATA_PAGE_PROCESSING_MODES)?;
    let modes: Vec<ProcessingMode> = value
        .split(',')
        .filter_map(deserialize_processing_mode)
        .collect();

    if modes.len() == document.pages.len() {
        Some(modes)
    } else {
        None
    }
}

fn serialize_processing_mode(mode: &ProcessingMode) -> &'static str {
    match mode {
        ProcessingMode::DocumentReconstruction => "document",
        ProcessingMode::VisualPreservation => "visual",
    }
}

fn deserialize_processing_mode(value: &str) -> Option<ProcessingMode> {
    match value {
        "document" => Some(ProcessingMode::DocumentReconstruction),
        "visual" => Some(ProcessingMode::VisualPreservation),
        _ => None,
    }
}

fn resumir_processing_mode(page_processing_modes: &[ProcessingMode]) -> ProcessingMode {
    if page_processing_modes.is_empty() {
        return ProcessingMode::DocumentReconstruction;
    }

    if page_processing_modes
        .iter()
        .all(|mode| *mode == ProcessingMode::VisualPreservation)
    {
        ProcessingMode::VisualPreservation
    } else {
        ProcessingMode::DocumentReconstruction
    }
}

fn infer_processing_mode_for_page(page: &Page) -> ProcessingMode {
    if seems_visual_page(page) {
        ProcessingMode::VisualPreservation
    } else {
        ProcessingMode::DocumentReconstruction
    }
}

fn seems_visual_page(page: &Page) -> bool {
    let page_area = (page.dimensions.width.max(1) as f64) * (page.dimensions.height.max(1) as f64);
    let mut total_image_area = 0.0f64;
    let mut max_image_area = 0.0f64;
    let mut total_text_area = 0.0f64;
    let mut text_blocks = 0usize;
    let mut wide_text_blocks = 0usize;
    let mut title_blocks = 0usize;
    let mut table_blocks = 0usize;

    for block in &page.blocks {
        let block_area = (block.bounding_box.width as f64) * (block.bounding_box.height as f64);
        let width_ratio = block.bounding_box.width as f64 / page.dimensions.width.max(1) as f64;

        match block.block_type {
            BlockType::Image => {
                total_image_area += block_area;
                max_image_area = max_image_area.max(block_area);
            }
            BlockType::Text | BlockType::List | BlockType::Formula => {
                total_text_area += block_area;
                text_blocks += 1;
                if width_ratio >= 0.55 {
                    wide_text_blocks += 1;
                }
            }
            BlockType::Title => {
                total_text_area += block_area;
                text_blocks += 1;
                title_blocks += 1;
                if width_ratio >= 0.55 {
                    wide_text_blocks += 1;
                }
            }
            BlockType::Table => {
                total_text_area += block_area;
                text_blocks += 1;
                table_blocks += 1;
                if width_ratio >= 0.55 {
                    wide_text_blocks += 1;
                }
            }
            _ => {}
        }
    }

    let total_image_ratio = total_image_area / page_area;
    let max_image_ratio = max_image_area / page_area;
    let total_text_ratio = total_text_area / page_area;
    let has_prominent_image = max_image_ratio >= 0.12 || total_image_ratio >= 0.18;
    let lacks_document_semantics = title_blocks == 0 && table_blocks == 0;
    let fragmented_text = text_blocks >= 2 && wide_text_blocks <= 1;
    let low_text_coverage = total_text_ratio <= 0.33;
    let raster_page = page.image_data.is_some();
    let looks_like_fragmented_ui = raster_page && text_blocks >= 3 && wide_text_blocks == 0;
    let many_non_documental_blocks = text_blocks >= 4 && wide_text_blocks <= 1;
    let lacks_body_text = text_blocks == 0 || wide_text_blocks == 0;

    ((has_prominent_image && lacks_document_semantics && (fragmented_text || low_text_coverage))
        || (looks_like_fragmented_ui && low_text_coverage)
        || (raster_page && lacks_document_semantics && many_non_documental_blocks)
        || (raster_page && has_prominent_image && lacks_body_text))
        && page_area > 0.0
}

fn order_blocks_for_document<'a>(blocks: &'a [Block], page_width: u32) -> Vec<&'a Block> {
    let mut sorted: Vec<&Block> = blocks.iter().collect();
    sorted.sort_by_key(|block| {
        (
            block.bounding_box.y,
            block.bounding_box.x,
            block.reading_order,
        )
    });

    if estimate_columns(blocks, page_width) == 1 {
        return sorted;
    }

    let mut anchors = Vec::new();
    let mut rest = Vec::new();
    for block in sorted {
        if is_visual_anchor(block, page_width) {
            anchors.push(block);
        } else {
            rest.push(block);
        }
    }

    let mut ordered = Vec::with_capacity(blocks.len());
    let mut segment_start = 0;

    for anchor in anchors {
        let mut segment = Vec::new();
        let mut pending = Vec::new();

        for block in rest {
            if block.bounding_box.y >= segment_start && block.bounding_box.y < anchor.bounding_box.y
            {
                segment.push(block);
            } else {
                pending.push(block);
            }
        }

        ordered.extend(order_segment_by_columns(segment, page_width));
        ordered.push(anchor);
        rest = pending;
        segment_start = anchor
            .bounding_box
            .y
            .saturating_add(anchor.bounding_box.height);
    }

    ordered.extend(order_segment_by_columns(rest, page_width));
    ordered
}

fn order_blocks_for_visual<'a>(blocks: &'a [Block]) -> Vec<&'a Block> {
    let mut sorted: Vec<&Block> = blocks.iter().collect();
    sorted.sort_by_key(|block| {
        (
            block.reading_order,
            block.bounding_box.y,
            block.bounding_box.x,
        )
    });
    sorted
}

fn order_segment_by_columns<'a>(blocks: Vec<&'a Block>, page_width: u32) -> Vec<&'a Block> {
    if !segment_looks_two_column(&blocks, page_width) {
        let mut linear = blocks;
        linear.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
        return linear;
    }

    let mut left = Vec::new();
    let mut right = Vec::new();
    let page_center = page_width / 2;

    for block in blocks {
        if center_x(block) <= page_center {
            left.push(block);
        } else {
            right.push(block);
        }
    }

    left.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
    right.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
    left.extend(right);
    left
}

fn estimate_columns(blocks: &[Block], page_width: u32) -> u32 {
    let blocks: Vec<&Block> = blocks.iter().collect();
    if segment_looks_two_column(&blocks, page_width) {
        2
    } else {
        1
    }
}

fn segment_looks_two_column(blocks: &[&Block], page_width: u32) -> bool {
    let page_width = page_width.max(1);
    let mut left = 0usize;
    let mut right = 0usize;
    let mut min_right_x = page_width;
    let mut max_left_x = 0u32;
    let mut top_left = u32::MAX;
    let mut top_right = u32::MAX;

    for block in blocks {
        if is_visual_anchor(block, page_width) {
            continue;
        }

        if !matches!(
            block.block_type,
            BlockType::Text | BlockType::List | BlockType::Table | BlockType::Formula
        ) {
            continue;
        }

        let center = center_x(block);
        if center <= page_width / 2 {
            left += 1;
            max_left_x = max_left_x.max(
                block
                    .bounding_box
                    .x
                    .saturating_add(block.bounding_box.width),
            );
            top_left = top_left.min(block.bounding_box.y);
        } else {
            right += 1;
            min_right_x = min_right_x.min(block.bounding_box.x);
            top_right = top_right.min(block.bounding_box.y);
        }
    }

    if left < 2 || right < 2 {
        return false;
    }

    let min_gutter = page_width.saturating_mul(4) / 100;
    let gutter = min_right_x.saturating_sub(max_left_x);
    let top_delta = top_left.abs_diff(top_right);
    gutter >= min_gutter && top_delta <= 220
}

fn infer_column_bases(blocks: &[&Block], page_width: u32) -> [u32; 2] {
    let mut left_base = page_width;
    let mut right_base = page_width;

    for block in blocks {
        if is_visual_anchor(block, page_width) {
            continue;
        }

        if center_x(block) <= page_width / 2 {
            left_base = left_base.min(block.bounding_box.x);
        } else {
            right_base = right_base.min(block.bounding_box.x);
        }
    }

    if left_base == page_width {
        left_base = 0;
    }
    if right_base == page_width {
        right_base = page_width / 2;
    }

    [left_base, right_base]
}

fn infer_spacing_before(
    blocks: &[&Block],
    current_index: usize,
    page_width: u32,
    total_columns: u32,
    column_index: u32,
) -> f32 {
    if current_index == 0 {
        return 0.0;
    }

    let current = blocks[current_index];

    for previous in blocks[..current_index].iter().rev() {
        let previous_columns = if is_visual_anchor(previous, page_width) {
            1
        } else {
            total_columns
        };
        let previous_column = block_column_index(previous, previous_columns, page_width);

        if previous_columns != total_columns || previous_column != column_index {
            continue;
        }

        let previous_bottom = previous
            .bounding_box
            .y
            .saturating_add(previous.bounding_box.height);
        let gap_px = current.bounding_box.y.saturating_sub(previous_bottom);
        return px_to_pt(gap_px).clamp(0.0, 32.0) as f32;
    }

    0.0
}

fn block_column_index(block: &Block, total_columns: u32, page_width: u32) -> u32 {
    if total_columns == 2 && center_x(block) > page_width / 2 {
        1
    } else {
        0
    }
}

fn infer_indent_pt(
    block: &Block,
    column_index: u32,
    total_columns: u32,
    column_bases: &[u32; 2],
) -> f32 {
    if total_columns == 1 {
        return 0.0;
    }

    let base_x = column_bases[column_index as usize];
    px_to_pt(block.bounding_box.x.saturating_sub(base_x)) as f32
}

fn px_to_pt(px: u32) -> f64 {
    (px as f64) * (72.0 / 150.0)
}

fn build_element(
    page: &Page,
    resolved: &ResolvedBlock,
    reading_order: u32,
    column_index: u32,
    total_columns: u32,
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
) -> ElementBlueprint {
    let (text, table, image_crop) = match &resolved.content {
        BlockContent::Text(text) => (text.clone(), None, None),
        BlockContent::Table(table) => (table.to_plain_text(), Some(table.clone()), None),
        BlockContent::Image(crop) | BlockContent::Raster(crop) => {
            (String::new(), None, Some(crop.clone()))
        }
        BlockContent::Empty => (String::new(), None, None),
    };

    ElementBlueprint {
        role: resolved.role,
        bounding_box: resolved.detected.bounding_box.clone(),
        reading_order,
        column_index,
        total_columns,
        text,
        ocr_confidence: resolved.ocr_confidence,
        layout_confidence: resolved.detected.layout_confidence,
        suspected_header: false,
        suspected_footer: false,
        table,
        image_crop,
        style: infer_style(
            page,
            resolved,
            total_columns,
            processing_mode,
            spacing_before_pt,
            left_indent_pt,
        ),
    }
}

fn infer_style(
    page: &Page,
    resolved: &ResolvedBlock,
    total_columns: u32,
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
) -> StyleHints {
    let page_width = page.dimensions.width.max(1);
    let page_height = page.dimensions.height.max(1);
    let width_ratio = resolved.detected.bounding_box.width as f32 / page_width as f32;
    let height_ratio = resolved.detected.bounding_box.height as f32 / page_height as f32;
    let page_center = page_width as f32 / 2.0;
    let block_center = center_x_resolved(resolved) as f32;
    let centered = (block_center - page_center).abs() <= (page_width as f32 * 0.12);

    let alignment = if width_ratio >= 0.85 {
        AlignmentHint::FullWidth
    } else if matches!(resolved.role, ElementRole::Title) && centered {
        AlignmentHint::Center
    } else if resolved.detected.bounding_box.x >= page_width.saturating_mul(55) / 100
        && width_ratio < 0.40
    {
        AlignmentHint::Right
    } else {
        AlignmentHint::Left
    };

    let emphasis = match resolved.role {
        ElementRole::Title => EmphasisHint::Strong,
        ElementRole::Separator | ElementRole::Stamp => EmphasisHint::Neutral,
        _ => EmphasisHint::Regular,
    };

    let font_scale = match resolved.role {
        ElementRole::Title => (1.2 + height_ratio * 14.0).clamp(1.4, 2.4),
        ElementRole::Paragraph | ElementRole::ListItem => {
            (0.9 + height_ratio * 6.0).clamp(0.9, 1.2)
        }
        ElementRole::Table => 0.95,
        ElementRole::Formula => 1.1,
        _ => 1.0,
    };

    let preserve_positioning = processing_mode == ProcessingMode::VisualPreservation
        || matches!(
            resolved.content,
            BlockContent::Image(_) | BlockContent::Raster(_) | BlockContent::Table(_)
        )
        || matches!(
            resolved.role,
            ElementRole::Figure
                | ElementRole::Formula
                | ElementRole::Signature
                | ElementRole::Stamp
                | ElementRole::Separator
        )
        || total_columns > 1;

    StyleHints {
        alignment,
        emphasis,
        font_scale,
        spacing_before_pt,
        left_indent_pt,
        keep_with_next: matches!(resolved.role, ElementRole::Title | ElementRole::Separator),
        preserve_positioning,
    }
}

fn reorder_page_document(page: &mut Page) {
    if page.blocks.len() <= 1 {
        renumber_reading_order(&mut page.blocks);
        return;
    }

    let ordered = order_blocks_for_document(&page.blocks, page.dimensions.width);
    page.blocks = ordered.into_iter().cloned().collect();
    renumber_reading_order(&mut page.blocks);
}

fn preserve_page_visual_order(page: &mut Page) {
    page.blocks.sort_by_key(|block| {
        (
            block.reading_order,
            block.bounding_box.y,
            block.bounding_box.x,
        )
    });
    renumber_reading_order(&mut page.blocks);
}

fn renumber_reading_order(blocks: &mut [Block]) {
    for (index, block) in blocks.iter_mut().enumerate() {
        block.reading_order = index as u32;
    }
}

fn is_visual_anchor(block: &Block, page_width: u32) -> bool {
    if matches!(block.block_type, BlockType::Title | BlockType::Separator) {
        return true;
    }

    let width_ratio = block.bounding_box.width as f32 / page_width.max(1) as f32;
    width_ratio >= 0.70
}

fn center_x(block: &Block) -> u32 {
    block
        .bounding_box
        .x
        .saturating_add(block.bounding_box.width / 2)
}

fn center_x_resolved(block: &ResolvedBlock) -> u32 {
    block
        .detected
        .bounding_box
        .x
        .saturating_add(block.detected.bounding_box.width / 2)
}

fn marcar_hints_encabezado_pie(pages: &mut [PageBlueprint]) {
    if pages.len() < 2 {
        return;
    }

    for page_index in 0..pages.len() {
        let (previous_pages, current_and_rest) = pages.split_at_mut(page_index);
        let Some((current_page, next_pages)) = current_and_rest.split_first_mut() else {
            continue;
        };

        if current_page.processing_mode != ProcessingMode::DocumentReconstruction {
            continue;
        }

        let previous_page = previous_pages
            .iter()
            .rev()
            .find(|page| page.processing_mode == ProcessingMode::DocumentReconstruction);
        let next_page = next_pages
            .iter()
            .find(|page| page.processing_mode == ProcessingMode::DocumentReconstruction);
        let height = current_page.dimensions.height.max(1);

        for element_index in 0..current_page.elements.len() {
            let element = &current_page.elements[element_index];
            let is_header = is_header_footer_candidate(element, height, true)
                && (previous_page
                    .is_some_and(|neighbor| has_repeated_match(element, neighbor, true))
                    || next_page
                        .is_some_and(|neighbor| has_repeated_match(element, neighbor, true)));
            let is_footer = is_header_footer_candidate(element, height, false)
                && (previous_page
                    .is_some_and(|neighbor| has_repeated_match(element, neighbor, false))
                    || next_page
                        .is_some_and(|neighbor| has_repeated_match(element, neighbor, false)));

            let element_mut = &mut current_page.elements[element_index];
            element_mut.suspected_header = is_header;
            element_mut.suspected_footer = is_footer;
        }
    }
}

fn is_header_footer_candidate(
    element: &ElementBlueprint,
    page_height: u32,
    is_header: bool,
) -> bool {
    if !matches!(
        element.role,
        ElementRole::Title
            | ElementRole::Paragraph
            | ElementRole::ListItem
            | ElementRole::Separator
    ) {
        return false;
    }

    let normalized_text = normalize_repeated_text(&element.text);
    if normalized_text.is_empty() {
        return false;
    }

    let top = element.bounding_box.y;
    let bottom = element
        .bounding_box
        .y
        .saturating_add(element.bounding_box.height);

    if is_header {
        top <= page_height.saturating_mul(14) / 100
    } else {
        bottom >= page_height.saturating_mul(88) / 100
    }
}

fn has_repeated_match(
    candidate: &ElementBlueprint,
    neighbor: &PageBlueprint,
    is_header: bool,
) -> bool {
    let neighbor_height = neighbor.dimensions.height.max(1);
    neighbor.elements.iter().any(|other| {
        is_header_footer_candidate(other, neighbor_height, is_header)
            && same_repeated_pattern(candidate, other, neighbor.dimensions.width.max(1))
    })
}

fn same_repeated_pattern(
    left: &ElementBlueprint,
    right: &ElementBlueprint,
    page_width: u32,
) -> bool {
    if left.role != right.role
        || left.column_index != right.column_index
        || left.total_columns != right.total_columns
    {
        return false;
    }

    let tolerance_x = page_width.saturating_mul(5) / 100;
    let tolerance_w = page_width.saturating_mul(8) / 100;
    let tolerance_y = 36;

    if left.bounding_box.x.abs_diff(right.bounding_box.x) > tolerance_x
        || left.bounding_box.width.abs_diff(right.bounding_box.width) > tolerance_w
        || left.bounding_box.y.abs_diff(right.bounding_box.y) > tolerance_y
    {
        return false;
    }

    let left_text = normalize_repeated_text(&left.text);
    let right_text = normalize_repeated_text(&right.text);
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }

    left_text == right_text || textual_similarity(&left_text, &right_text) >= 0.86
}

fn normalize_repeated_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_space = false;

    for character in text.chars().flat_map(char::to_lowercase) {
        let emitted = if character.is_ascii_digit() {
            Some('#')
        } else if character.is_alphanumeric() {
            Some(character)
        } else if character.is_whitespace()
            || matches!(character, '-' | '–' | '—' | '_' | '/' | ':')
        {
            Some(' ')
        } else {
            None
        };

        match emitted {
            Some(' ') if !previous_space => {
                normalized.push(' ');
                previous_space = true;
            }
            Some(' ') => {}
            Some(value) => {
                normalized.push(value);
                previous_space = false;
            }
            None => {}
        }
    }

    normalized.trim().to_string()
}

fn textual_similarity(left: &str, right: &str) -> f32 {
    if left == right {
        return 1.0;
    }

    let left_len = left.chars().count();
    let right_len = right.chars().count();
    if left_len == 0 || right_len == 0 {
        return 0.0;
    }

    let distance = levenshtein_distance(left, right) as f32;
    let max_len = left_len.max(right_len) as f32;
    1.0 - (distance / max_len)
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0usize; right_chars.len() + 1];

    for (i, left_char) in left_chars.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = if left_char == right_char { 0 } else { 1 };
            current[j + 1] = (current[j] + 1)
                .min(previous[j + 1] + 1)
                .min(previous[j] + substitution_cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}
