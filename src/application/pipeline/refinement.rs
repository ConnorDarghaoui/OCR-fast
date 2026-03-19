use crate::domain::{Document, DocumentBlueprint, ProcessingProfile};
use std::path::Path;

/// Error propagable desde un pass de refinamiento hacia la orquestación.
pub type RefinementError = Box<dyn std::error::Error + Send + Sync>;

/// Etapas estables donde el pipeline permite refinamientos opcionales.
///
/// El enum evita que cada pass decida por sí solo dónde conectarse. La
/// orquestación sigue controlando el orden global, y cada implementación declara
/// explícitamente la frontera donde aporta valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefinementStage {
    /// Corre sobre el documento parseado y preparado, antes del OCR principal.
    BeforeOcr,
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

/// `refinement.rs` queda reservado para hooks opcionales del pipeline.
///
/// La limpieza raster canónica vive en `PreprocessorPort` y las recuperaciones
/// OCR costosas viven en `application::pipeline::recovery`. Este módulo ya no
/// debe duplicar transformaciones de imagen ni rutas de recuperación.

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
