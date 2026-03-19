use crate::domain::{Block, BlockType, Document, DocumentBlueprint, ProcessingProfile, Rectangle};
use crate::infrastructure::preprocessors::ImagePreprocessor;
use crate::interfaces::ports::{OcrEnginePort, PreprocessorPort};
use std::path::Path;
use std::sync::Arc;

/// Error propagable desde un pass de refinamiento hacia la orquestación.
pub type RefinementError = Box<dyn std::error::Error + Send + Sync>;

/// Etapas estables donde el pipeline permite refinamientos opcionales.
///
/// El enum evita que cada pass decida por sí solo dónde conectarse. La
/// orquestación sigue controlando el orden global, y cada implementación declara
/// explícitamente la frontera donde aporta valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefinementStage {
    /// Corre sobre el documento parseado y preprocesado, antes de layout.
    BeforeLayout,
    /// Corre tras OCR, antes de tablas y postproceso.
    AfterOcr,
    /// Corre tras OCR, tablas y postproceso, antes de construir el blueprint.
    BeforeBlueprint,
    /// Corre sobre el documento ya acompañado por un blueprint visual.
    AfterBlueprint,
}

/// Presupuesto máximo de passes permitidos en una corrida del pipeline.
///
/// La intención es evitar que una cadena de refinamientos crezca sin control y
/// convierta una mejora marginal en latencia no acotada. El presupuesto se mide
/// en número de passes ejecutados, no en tiempo, porque es la restricción más
/// simple y reproducible para una primera versión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinementBudget {
    /// Número máximo de passes que el pipeline permitirá ejecutar.
    pub max_passes: usize,
}

impl RefinementBudget {
    /// Construye un presupuesto explícito de passes.
    pub fn new(max_passes: usize) -> Self {
        Self { max_passes }
    }

    /// Indica si todavía hay cupo para ejecutar otro pass.
    pub fn allows(self, consumed_passes: usize) -> bool {
        consumed_passes < self.max_passes
    }

    /// Retorna el cupo restante antes de ejecutar el siguiente pass.
    pub fn remaining(self, consumed_passes: usize) -> usize {
        self.max_passes.saturating_sub(consumed_passes)
    }
}

impl Default for RefinementBudget {
    fn default() -> Self {
        Self { max_passes: 4 }
    }
}

/// Contexto inmutable visible para cada pass de refinamiento.
///
/// El contexto expone solo metadatos estables de la corrida actual: ubicación
/// del archivo, perfil, cantidad de páginas, etapa activa y presupuesto
/// restante. Dejarlo inmutable simplifica testing y evita que passes aislados
/// compitan por mutar estado compartido fuera del documento o blueprint.
#[derive(Debug, Clone)]
pub struct RefinementContext<'a> {
    /// Ruta física del documento de entrada.
    pub source_path: &'a Path,
    /// Perfil operativo solicitado para la corrida actual.
    pub profile: &'a ProcessingProfile,
    /// Cantidad total de páginas visibles por el pipeline.
    pub total_pages: u32,
    /// Etapa estable donde el pass está siendo ejecutado.
    pub stage: RefinementStage,
    /// Número de passes ya consumidos antes del actual.
    pub consumed_passes: usize,
    /// Cupo restante de passes, incluyendo el pass actual.
    pub remaining_passes: usize,
}

/// Contrato para refinamientos encadenables sobre documento y blueprint.
///
/// El trait está deliberadamente centrado en mutación in-place de `Document` y
/// `Option<DocumentBlueprint>` para soportar tanto passes previos al blueprint
/// como pases posteriores sin duplicar contratos. Un pass puede ignorar el
/// blueprint cuando aún no exista, o rechazar la ejecución si su semántica lo
/// exige.
///
/// # Trade-offs
///
/// El diseño evita genéricos complejos y mantiene el pipeline composable con
/// `Arc<dyn RefinementPass>`. A cambio, el contrato es menos restrictivo: una
/// implementación mal diseñada podría intentar actuar en una etapa donde no
/// tiene sentido. Esa validación queda explícita en `stage()`.
pub trait RefinementPass: Send + Sync {
    /// Etapa del pipeline donde este pass debe ejecutarse.
    fn stage(&self) -> RefinementStage;

    /// Nombre estable para logs, diagnóstico y pruebas.
    fn name(&self) -> &str;

    /// Aplica un refinamiento opcional sobre el documento y/o blueprint.
    ///
    /// # Errors
    ///
    /// Un pass debe fallar cuando no pueda preservar consistencia del
    /// documento o del blueprint. El pipeline propagará ese error como fallo de
    /// la corrida, en lugar de ocultarlo y dejar un estado ambiguo.
    fn refine(
        &self,
        document: &mut Document,
        blueprint: &mut Option<DocumentBlueprint>,
        context: &RefinementContext<'_>,
    ) -> Result<(), RefinementError>;
}

/// Pass inerte para validar cableado y presupuesto sin alterar la salida.
///
/// Su utilidad es arquitectónica: permite verificar que el pipeline ejecuta la
/// cadena de refinamiento en la etapa esperada antes de introducir passes caros
/// o con efectos visibles sobre el documento final.
pub struct NoopRefinementPass {
    stage: RefinementStage,
}

impl NoopRefinementPass {
    /// Construye un pass inerte para una etapa concreta.
    pub fn at_stage(stage: RefinementStage) -> Self {
        Self { stage }
    }
}

impl Default for NoopRefinementPass {
    fn default() -> Self {
        Self {
            stage: RefinementStage::AfterBlueprint,
        }
    }
}

impl RefinementPass for NoopRefinementPass {
    fn stage(&self) -> RefinementStage {
        self.stage
    }

    fn name(&self) -> &str {
        "NoopRefinementPass"
    }

    fn refine(
        &self,
        _document: &mut Document,
        _blueprint: &mut Option<DocumentBlueprint>,
        _context: &RefinementContext<'_>,
    ) -> Result<(), RefinementError> {
        Ok(())
    }
}

/// Pass de limpieza suave para reducir ruido previo a layout y OCR.
///
/// La implementación reutiliza el preprocesador raster del sistema con una
/// configuración restringida solo a denoise. Mantenerlo como pass separado
/// permite activarlo únicamente en corridas donde el coste extra se justifica.
pub struct DenoisePass {
    preprocessor: Arc<dyn PreprocessorPort>,
}

impl DenoisePass {
    /// Construye el pass con la política de ruido por defecto del producto.
    pub fn new() -> Self {
        Self::with_preprocessor(Arc::new(ImagePreprocessor::with_config(
            false, false, true, 300,
        )))
    }

    /// Inyecta un preprocesador alternativo para pruebas o tuning avanzado.
    pub fn with_preprocessor(preprocessor: Arc<dyn PreprocessorPort>) -> Self {
        Self { preprocessor }
    }
}

impl Default for DenoisePass {
    fn default() -> Self {
        Self::new()
    }
}

impl RefinementPass for DenoisePass {
    fn stage(&self) -> RefinementStage {
        RefinementStage::BeforeLayout
    }

    fn name(&self) -> &str {
        "DenoisePass"
    }

    fn refine(
        &self,
        document: &mut Document,
        _blueprint: &mut Option<DocumentBlueprint>,
        _context: &RefinementContext<'_>,
    ) -> Result<(), RefinementError> {
        self.preprocessor.preprocess(document)?;
        Ok(())
    }
}

/// Pass de corrección geométrica para reducir inclinación global de página.
///
/// Deskew corre antes de layout porque un error angular pequeño deforma la
/// detección de columnas, líneas y tablas para todas las etapas posteriores.
pub struct DeskewPass {
    preprocessor: Arc<dyn PreprocessorPort>,
}

impl DeskewPass {
    /// Construye el pass con la política de deskew por defecto del producto.
    pub fn new() -> Self {
        Self::with_preprocessor(Arc::new(ImagePreprocessor::with_config(
            false, true, false, 300,
        )))
    }

    /// Inyecta un preprocesador alternativo para pruebas o tuning avanzado.
    pub fn with_preprocessor(preprocessor: Arc<dyn PreprocessorPort>) -> Self {
        Self { preprocessor }
    }
}

impl Default for DeskewPass {
    fn default() -> Self {
        Self::new()
    }
}

impl RefinementPass for DeskewPass {
    fn stage(&self) -> RefinementStage {
        RefinementStage::BeforeLayout
    }

    fn name(&self) -> &str {
        "DeskewPass"
    }

    fn refine(
        &self,
        document: &mut Document,
        _blueprint: &mut Option<DocumentBlueprint>,
        _context: &RefinementContext<'_>,
    ) -> Result<(), RefinementError> {
        self.preprocessor.preprocess(document)?;
        Ok(())
    }
}

/// Pass de reintento OCR para bloques débiles antes de construir el blueprint.
///
/// El pass no reescribe la geometría del documento original. Reprocesa una copia
/// completa con un perfil más preciso y fusiona solo bloques cuya confianza OCR
/// mejora de forma material, preservando el layout aceptado por el pipeline.
///
/// # Trade-offs
///
/// La estrategia actual reejecuta OCR sobre el documento completo porque el
/// puerto disponible no expone OCR selectivo por bloque. Eso aumenta latencia
/// frente a una solución quirúrgica, pero permite introducir mejora selectiva
/// sin romper contratos ya consumidos por TUI y exportadores.
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

impl ConfidenceBoostPass {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Dimensions, Page};
    use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Rgb};
    use std::collections::HashMap;
    use std::io::Cursor;

    fn documento_con_imagen(image_data: Vec<u8>) -> Document {
        Document {
            id: "doc".to_string(),
            source_path: Path::new("/tmp/doc.png").to_path_buf(),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 64,
                    height: 64,
                },
                blocks: vec![],
                image_data: Some(image_data),
            }],
            metadata: HashMap::new(),
        }
    }

    fn png_desde_imagen(imagen: DynamicImage) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        imagen
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("Debe serializar imagen de prueba");
        buffer.into_inner()
    }

    fn imagen_ruidosa_png() -> Vec<u8> {
        let mut image = ImageBuffer::from_pixel(64, 64, Rgb([255, 255, 255]));
        for x in 12..52 {
            image.put_pixel(x, 30, Rgb([0, 0, 0]));
        }
        for (x, y) in [(4, 5), (8, 42), (18, 10), (40, 44), (54, 18)] {
            image.put_pixel(x, y, Rgb([0, 0, 0]));
        }
        png_desde_imagen(DynamicImage::ImageRgb8(image))
    }

    fn imagen_inclinada_png() -> Vec<u8> {
        let mut image = GrayImage::from_pixel(96, 64, Luma([255]));
        for offset in 0..55 {
            let x = 12 + offset;
            let y = 14 + (offset / 6);
            if x < 96 && y < 64 {
                image.put_pixel(x, y, Luma([0]));
            }
            if x < 96 && y + 10 < 64 {
                image.put_pixel(x, y + 10, Luma([0]));
            }
        }
        png_desde_imagen(DynamicImage::ImageLuma8(image))
    }

    fn contexto(stage: RefinementStage) -> RefinementContext<'static> {
        RefinementContext {
            source_path: Path::new("/tmp/doc.png"),
            profile: &ProcessingProfile::Balanced,
            total_pages: 1,
            stage,
            consumed_passes: 0,
            remaining_passes: 1,
        }
    }

    #[test]
    fn test_denoise_pass_reescribe_raster() {
        let original = imagen_ruidosa_png();
        let mut document = documento_con_imagen(original.clone());
        let mut blueprint = None;

        DenoisePass::new()
            .refine(
                &mut document,
                &mut blueprint,
                &contexto(RefinementStage::BeforeLayout),
            )
            .expect("Denoise debe completar");

        let procesada = document.pages[0]
            .image_data
            .clone()
            .expect("La imagen debe seguir disponible");
        assert_ne!(procesada, original);

        let decoded = image::load_from_memory(&procesada).expect("PNG procesado válido");
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 64);
    }

    #[test]
    fn test_deskew_pass_reescribe_raster() {
        let original = imagen_inclinada_png();
        let mut document = documento_con_imagen(original.clone());
        let mut blueprint = None;

        DeskewPass::new()
            .refine(
                &mut document,
                &mut blueprint,
                &contexto(RefinementStage::BeforeLayout),
            )
            .expect("Deskew debe completar");

        let procesada = document.pages[0]
            .image_data
            .clone()
            .expect("La imagen debe seguir disponible");
        assert_ne!(procesada, original);

        let decoded = image::load_from_memory(&procesada).expect("PNG procesado válido");
        assert_eq!(decoded.width(), 96);
        assert_eq!(decoded.height(), 64);
    }
}
