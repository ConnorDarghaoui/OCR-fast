use crate::domain::{
    BlockType, Document, Page, ProcessingMode, DOCUMENT_METADATA_PAGE_PROCESSING_MODES,
    DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE,
};

pub(crate) fn resumir_processing_mode(page_processing_modes: &[ProcessingMode]) -> ProcessingMode {
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
