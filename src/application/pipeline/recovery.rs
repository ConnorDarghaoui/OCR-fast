use crate::domain::{Block, BlockType, Document, DocumentBlueprint, ProcessingProfile, Rectangle};
use crate::interfaces::ports::OcrEnginePort;
use std::sync::Arc;

use super::refinement::{RefinementContext, RefinementError, RefinementPass, RefinementStage};

/// Pass avanzado de reintento OCR para bloques débiles.
///
/// Este pass no forma parte del camino principal del producto. Se mantiene como
/// mecanismo opt-in de recuperación cuando el caller acepta el coste extra de
/// reprocesar páginas débiles con un perfil más preciso.
///
/// # Ownership
///
/// `ConfidenceBoostPass` no corrige geometría, no ordena páginas y no reemplaza
/// el preprocesamiento raster. Su única responsabilidad es intentar mejorar el
/// contenido OCR de bloques ya segmentados cuya confianza quedó por debajo del
/// umbral configurado.
pub struct ConfidenceBoostPass {
    ocr_engine: Arc<dyn OcrEnginePort>,
    threshold: f64,
    min_gain: f64,
    retry_profile: ProcessingProfile,
}

impl ConfidenceBoostPass {
    /// Construye el pass con un reintento OCR conservador y perfil preciso.
    pub fn new(ocr_engine: Arc<dyn OcrEnginePort>) -> Self {
        Self::with_config(ocr_engine, 0.78, 0.05, ProcessingProfile::Accurate)
    }

    /// Construye el pass con umbrales explícitos para tuning o pruebas.
    pub fn with_config(
        ocr_engine: Arc<dyn OcrEnginePort>,
        threshold: f64,
        min_gain: f64,
        retry_profile: ProcessingProfile,
    ) -> Self {
        Self {
            ocr_engine,
            threshold,
            min_gain,
            retry_profile,
        }
    }

    fn has_low_confidence_blocks(&self, document: &Document) -> bool {
        document.pages.iter().any(|page| {
            page.blocks
                .iter()
                .any(|block| is_confidence_eligible(block) && block.confidence < self.threshold)
        })
    }

    fn merge_retry_results(&self, original: &mut Document, retried: &Document) {
        for (original_page, retried_page) in original.pages.iter_mut().zip(&retried.pages) {
            let mut candidate_used = vec![false; retried_page.blocks.len()];

            for original_block in &mut original_page.blocks {
                if !is_confidence_eligible(original_block)
                    || original_block.confidence >= self.threshold
                {
                    continue;
                }

                let Some((candidate_index, candidate_block)) =
                    best_retry_candidate(original_block, &retried_page.blocks, &candidate_used)
                else {
                    continue;
                };

                if candidate_block.confidence < original_block.confidence + self.min_gain
                    || candidate_block.content.trim().is_empty()
                {
                    continue;
                }

                original_block.content = candidate_block.content.clone();
                original_block.confidence = candidate_block.confidence;
                original_block.table_structure = candidate_block.table_structure.clone();
                original_block.embedded_image = candidate_block.embedded_image.clone();
                if original_block.layout_confidence.is_none() {
                    original_block.layout_confidence = candidate_block.layout_confidence;
                }
                candidate_used[candidate_index] = true;
            }
        }
    }

    fn build_retry_document(&self, document: &Document) -> Document {
        let pages = document
            .pages
            .iter()
            .filter(|page| {
                page.blocks
                    .iter()
                    .any(|block| is_confidence_eligible(block) && block.confidence < self.threshold)
            })
            .cloned()
            .collect();

        Document {
            id: document.id.clone(),
            source_path: document.source_path.clone(),
            pages,
            metadata: document.metadata.clone(),
        }
    }
}

impl RefinementPass for ConfidenceBoostPass {
    fn stage(&self) -> RefinementStage {
        RefinementStage::AfterOcr
    }

    fn name(&self) -> &str {
        "ConfidenceBoostPass"
    }

    fn refine(
        &self,
        document: &mut Document,
        _blueprint: &mut Option<DocumentBlueprint>,
        _context: &RefinementContext<'_>,
    ) -> Result<(), RefinementError> {
        if !self.has_low_confidence_blocks(document) {
            return Ok(());
        }

        let mut retried = self.build_retry_document(document);
        self.ocr_engine.process(&mut retried, &self.retry_profile)?;
        self.merge_retry_results(document, &retried);
        Ok(())
    }
}

fn is_confidence_eligible(block: &Block) -> bool {
    matches!(
        block.block_type,
        BlockType::Text | BlockType::Title | BlockType::List | BlockType::Table
    )
}

fn best_retry_candidate<'a>(
    original: &Block,
    candidates: &'a [Block],
    used: &[bool],
) -> Option<(usize, &'a Block)> {
    candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            !used[*index]
                && candidate.block_type == original.block_type
                && block_similarity_score(original, candidate) >= 0.55
        })
        .max_by(|(_, left), (_, right)| {
            block_similarity_score(original, left)
                .partial_cmp(&block_similarity_score(original, right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn block_similarity_score(left: &Block, right: &Block) -> f64 {
    let mut score = rectangle_iou(&left.bounding_box, &right.bounding_box);
    if left.reading_order == right.reading_order {
        score += 0.35;
    }
    score
}

fn rectangle_iou(left: &Rectangle, right: &Rectangle) -> f64 {
    let left_x2 = left.x.saturating_add(left.width);
    let left_y2 = left.y.saturating_add(left.height);
    let right_x2 = right.x.saturating_add(right.width);
    let right_y2 = right.y.saturating_add(right.height);

    let intersection_x1 = left.x.max(right.x);
    let intersection_y1 = left.y.max(right.y);
    let intersection_x2 = left_x2.min(right_x2);
    let intersection_y2 = left_y2.min(right_y2);

    if intersection_x2 <= intersection_x1 || intersection_y2 <= intersection_y1 {
        return 0.0;
    }

    let intersection_area =
        (intersection_x2 - intersection_x1) as f64 * (intersection_y2 - intersection_y1) as f64;
    let left_area = left.width as f64 * left.height as f64;
    let right_area = right.width as f64 * right.height as f64;
    let union_area = left_area + right_area - intersection_area;

    if union_area <= f64::EPSILON {
        0.0
    } else {
        intersection_area / union_area
    }
}
