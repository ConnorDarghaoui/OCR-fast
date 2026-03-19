use crate::domain::{Document, DocumentBlueprint, ProcessingProfile};
use crate::infrastructure::preprocessors::ImagePreprocessor;
use crate::interfaces::ports::PreprocessorPort;
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

/// `refinement.rs` queda reservado para limpieza y transformaciones de página.
///
/// Los mecanismos caros de recuperación OCR no viven aquí más. Para ese caso se
/// debe usar `application::pipeline::recovery`, que deja explícito que se trata
/// de una ruta opt-in y no del camino principal del producto.

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
