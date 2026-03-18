use crate::domain::errors::{
    DocumentError, ExportError, JobStoreError, LayoutError, ModelDownloadError, OcrError,
    PreprocessError,
};
use crate::domain::{Block, Document, DocumentBlueprint, Job, Page, ProcessingProfile};
use std::path::Path;
use std::sync::Arc;

/// Contrato para rasterizar páginas PDF sin acoplar el pipeline a Pdfium.
///
/// El trait opera por página para mantener memoria acotada y para evitar que la
/// capa de aplicación necesite conocer detalles de paginación incremental del
/// backend. `Send + Sync` es obligatorio porque la instancia puede compartirse
/// entre la TUI y workers de procesamiento vía `Arc<dyn Trait>`.
///
/// # Performance
///
/// La granularidad por página evita materializar el documento completo en RAM.
///
/// # Trade-offs
///
/// El contrato no expone tuning de DPI ni caching; eso simplifica la API pública
/// pero deja esas decisiones a la implementación concreta.
pub trait PdfRendererPort: Send + Sync {
    /// Rasteriza una página a imagen lista para etapas posteriores del pipeline.
    ///
    /// # Errors
    ///
    /// Retorna `DocumentError` si el archivo no existe, no es PDF válido o el
    /// backend de render no puede decodificar la página solicitada.
    fn render_page(
        &self,
        path: &Path,
        page_number: u32,
    ) -> Result<image::DynamicImage, DocumentError>;

    /// Cuenta páginas sin obligar a renderizar el documento completo.
    fn get_page_count(&self, path: &Path) -> Result<u32, DocumentError>;
}

/// Contrato para segmentar una página en bloques semánticos ordenados.
///
/// El objetivo del puerto es aislar al pipeline de la estrategia concreta de
/// layout: heurística, neuronal o híbrida. La interfaz exige orden de lectura
/// estable porque exportadores y OCR posteriores dependen de esa secuencia para
/// producir resultados reproducibles.
///
/// # Trade-offs
///
/// El retorno usa `Vec<Block>` en lugar de un iterador streaming porque etapas
/// posteriores necesitan acceso repetido y mutación de los bloques generados.
pub trait LayoutEnginePort: Send + Sync {
    /// Analiza una página y retorna bloques con geometría y semántica inicial.
    fn analyze(&self, page: &Page) -> Result<Vec<Block>, LayoutError>;

    /// Identificador estable del motor para logs, telemetría y diagnóstico.
    fn name(&self) -> &str;
}

/// Contrato para poblar contenido OCR sobre un documento ya estructurado.
///
/// El puerto trabaja sobre `&mut Document` para evitar reconstrucciones
/// intermedias y porque el OCR completa información sobre bloques ya existentes.
/// Esa decisión es más eficiente que devolver un árbol nuevo, aunque obliga a una
/// disciplina clara de mutación durante el pipeline.
///
/// # Concurrency
///
/// `Send + Sync` permite compartir engines pesados mediante `Arc`. El contrato,
/// sin embargo, no garantiza interior mutability lock-free; cada implementación
/// decide su estrategia de sincronización y caching.
pub trait OcrEnginePort: Send + Sync {
    /// Ejecuta OCR y escribe resultados sobre los bloques del documento.
    ///
    /// # Errors
    ///
    /// Retorna `OcrError` cuando el backend no está listo, no soporta el idioma
    /// requerido o falla durante inferencia/carga de modelos.
    fn process(&self, document: &mut Document, profile: &ProcessingProfile)
        -> Result<(), OcrError>;

    /// Identificador estable del engine para trazabilidad operacional.
    fn name(&self) -> &str;

    /// Indica si el engine ya integra layout y OCR en una sola inferencia.
    ///
    /// # Trade-offs
    ///
    /// Exponer esta capacidad evita trabajo duplicado, pero introduce una rama de
    /// orquestación adicional en el pipeline. Se mantiene como booleano simple para
    /// no contaminar el dominio con detalles del backend.
    fn provides_layout(&self) -> bool {
        false
    }
}

/// Contrato para transformar archivos fuente en un `Document` de dominio.
///
/// El parser es responsable de normalizar formatos heterogéneos a una estructura
/// común consumible por layout, OCR y exportadores. Esa normalización temprana
/// desacopla el resto del sistema de diferencias entre PDF raster, imagen simple
/// o formatos futuros.
pub trait DocumentParserPort: Send + Sync {
    /// Parsea un recurso físico y construye la representación de dominio inicial.
    fn parse(&self, path: &Path) -> Result<Document, DocumentError>;
}

/// Contrato de persistencia duradera para snapshots de `Job`.
///
/// La API es síncrona a propósito: el almacenamiento actual vive en disco local y
/// la TUI coordina sus propios hilos de background. Introducir async aquí
/// complejizaría lifetimes, testing y ergonomía sin beneficio inmediato.
pub trait JobStorePort: Send + Sync {
    /// Persiste un snapshot completo del trabajo.
    fn save(&self, job: &Job) -> Result<(), JobStoreError>;
    /// Recupera un trabajo por identificador lógico.
    fn get(&self, id: &str) -> Result<Job, JobStoreError>;
    /// Sobrescribe el snapshot usando `job.id` como clave de escritura.
    fn update(&self, job: &Job) -> Result<(), JobStoreError>;
    /// Lista el conjunto completo de trabajos visibles para la aplicación.
    fn list(&self) -> Result<Vec<Job>, JobStoreError>;
    /// Elimina un trabajo persistido y sus metadatos asociados.
    fn delete(&self, id: &str) -> Result<(), JobStoreError>;
}

/// Contrato para materializar un `Job` en un formato de salida específico.
///
/// Exportación se define como puerto aparte porque su ciclo de vida y fallos no
/// deben acoplarse al OCR mismo. Un trabajo puede ser válido aunque la escritura
/// del artefacto final falle.
pub trait ExporterPort: Send + Sync {
    /// Escribe el resultado del trabajo en la ruta de salida indicada.
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError>;
    /// Nombre estable del formato para UI, logs y resolución de exportador.
    fn format_name(&self) -> &str;
}

/// Contrato para resolver un motor de layout auxiliar según el OCR activo.
///
/// El puerto encapsula la política de composición entre OCR y layout para que la
/// TUI no tenga que decidir entre implementaciones concretas de infraestructura.
pub trait LayoutEngineFactoryPort: Send + Sync {
    /// Retorna un motor de layout auxiliar cuando el OCR activo no lo incorpora.
    fn create_for(&self, ocr_engine: &dyn OcrEnginePort) -> Option<Arc<dyn LayoutEnginePort>>;
}

/// Contrato para exportar un `Job` usando su formato configurado.
///
/// La capa de presentación delega la resolución de exportadores concretos a este
/// puerto para evitar acoplarse a tipos de infraestructura.
pub trait JobExporterPort: Send + Sync {
    /// Materializa el trabajo en la ruta de salida indicada.
    fn export_job(&self, job: &Job, output_path: &Path) -> Result<(), ExportError>;
}

/// Contrato para transformaciones raster previas al OCR.
///
/// Se modela como fase opcional porque el preprocesamiento mejora documentos
/// escaneados ruidosos, pero también puede degradar PDFs nativos o imágenes ya
/// limpias. Mantenerlo detrás de un puerto permite activar, reemplazar o anular
/// esta etapa sin tocar la orquestación.
pub trait PreprocessorPort: Send + Sync {
    /// Aplica transformaciones in-place sobre el documento.
    fn preprocess(&self, document: &mut Document) -> Result<(), PreprocessError>;
}

/// Contrato para correcciones posteriores a la inferencia OCR.
///
/// Esta fase agrupa normalización Unicode, corrección léxica y limpieza textual
/// sin reabrir dependencias con el engine. Mantenerla separada permite iterar la
/// calidad lingüística sin alterar inferencia ni layout.
pub trait PostprocessorPort: Send + Sync {
    /// Ajusta contenido textual ya reconocido sin modificar la geometría.
    fn postprocess(&self, document: &mut Document) -> Result<(), OcrError>;
}

/// Contrato para enriquecer bloques tabulares con estructura interna.
///
/// El puerto existe por separado del layout general porque detectar una tabla y
/// reconstruir su malla son problemas con costos y modelos distintos. Esa
/// separación permite apagar el análisis tabular cuando la latencia importa más.
pub trait TableAnalyzerPort: Send + Sync {
    /// Detecta estructura interna en bloques marcados como tabla.
    fn analyze_tables(&self, document: &mut Document) -> Result<(), LayoutError>;
    /// Nombre del analizador para observabilidad y soporte.
    fn name(&self) -> &str;
}

/// Contrato para reconstrucción visual posterior a OCR y layout.
///
/// El builder traduce el árbol crudo del dominio a una representación intermedia
/// orientada a exportadores ricos como DOCX, LaTeX o PDF reconstruido. Esta fase
/// existe para evitar que cada exportador reimplante heurísticas de columnas,
/// anclas visuales y preservación de imágenes.
///
/// # Trade-offs
///
/// El contrato opera sobre `Document` completo y no sobre páginas aisladas porque
/// el exportador necesita una vista coherente del documento final. Esa decisión
/// simplifica exportación a costa de impedir streaming total en esta frontera.
pub trait DocumentBlueprintBuilderPort: Send + Sync {
    /// Construye un blueprint visual listo para exportación de alta fidelidad.
    ///
    /// # Errors
    ///
    /// Retorna `LayoutError` cuando las invariantes geométricas del documento son
    /// insuficientes para producir un orden visual reproducible.
    fn build_blueprint(&self, document: &Document) -> Result<DocumentBlueprint, LayoutError>;

    /// Nombre estable del builder para trazabilidad y diagnóstico.
    fn name(&self) -> &str;
}

/// Contrato para ensamblar el documento final a partir del layout detectado.
///
/// Esta fase toma bloques ya segmentados y enriquecidos por OCR para imponer una
/// secuencia de lectura canónica antes de exportar o serializar resultados.
pub trait DocumentAssemblerPort: Send + Sync {
    /// Reordena y normaliza el documento para consumo final.
    fn assemble(&self, document: &mut Document) -> Result<(), LayoutError>;
    /// Identificador estable del ensamblador para logs y diagnóstico.
    fn name(&self) -> &str;
}

/// Contrato para asegurar disponibilidad de artefactos de inferencia.
///
/// Modelos ONNX son un problema operativo aparte: implican red, integridad,
/// almacenamiento local y políticas de reintento. Sacarlos a un puerto reduce el
/// acoplamiento entre inicialización del engine y gestión de artefactos.
pub trait ModelManagerPort: Send + Sync {
    /// Garantiza disponibilidad local de todos los modelos requeridos.
    ///
    /// # Errors
    ///
    /// Retorna `ModelDownloadError` si la red, el filesystem o la verificación de
    /// integridad impiden dejar el conjunto en estado consistente.
    fn ensure_models(&self) -> Result<std::path::PathBuf, ModelDownloadError>;
    /// Informa si un modelo nominal ya está disponible localmente.
    fn model_exists(&self, model_name: &str) -> bool;
}
