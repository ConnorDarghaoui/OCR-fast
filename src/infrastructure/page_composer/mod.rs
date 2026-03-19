mod header_footer;
mod mode;
mod ordering;
mod projection;

use crate::domain::errors::LayoutError;
use crate::domain::{Document, DocumentBlueprint, Page, PageBlueprint, ProcessingMode};
use crate::infrastructure::automata::BlockAutomata;
use header_footer::marcar_hints_encabezado_pie;
use mode::resumir_processing_mode;
pub use mode::{inferir_modos_procesamiento_por_pagina, persistir_modos_procesamiento_por_pagina};
use ordering::{
    estimate_columns, infer_column_bases, infer_indent_pt, infer_spacing_before, is_visual_anchor,
    order_blocks_for_document, order_blocks_for_visual, preserve_page_visual_order,
    reorder_page_document,
};
use projection::build_element;

/// Capa única de composición por página para renderizadores ricos.
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
            let column_index =
                if use_two_columns && center_is_right_column(block, page.dimensions.width) {
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

fn center_is_right_column(block: &crate::domain::Block, page_width: u32) -> bool {
    let center = block
        .bounding_box
        .x
        .saturating_add(block.bounding_box.width / 2);
    center > page_width / 2
}
