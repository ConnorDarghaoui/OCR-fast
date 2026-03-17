//! Contratos de infraestructura (Dependency Inversion).
//!
//! La capa de aplicacion depende solo de estos traits, nunca de implementaciones
//! concretas. Cada trait lleva `Send + Sync` para compartirse via `Arc<dyn Trait>`
//! entre el thread de la TUI y los threads de procesamiento.

use crate::domain::errors::{
    DocumentError, ExportError, JobStoreError, LayoutError, OcrError, PreprocessError,
    ModelDownloadError,
};
use crate::domain::{Block, Document, Job, Page, ProcessingProfile};
use std::path::Path;

/// Renderiza paginas de un PDF a imagen en memoria.
///
/// El renderizado por pagina individual mantiene el consumo de RAM constante
/// independientemente del tamaño del archivo.
pub trait PdfRendererPort: Send + Sync {
    /// Renderiza `page_number` a imagen. El objetivo de resolucion es 300 DPI.
    fn render_page(&self, path: &Path, page_number: u32) -> Result<image::DynamicImage, DocumentError>;

    /// Cuenta las paginas sin renderizar el documento completo.
    fn get_page_count(&self, path: &Path) -> Result<u32, DocumentError>;
}

/// Detecta regiones de contenido en una pagina (bloques, columnas, figuras).
///
/// Dos familias de implementacion: heuristica (XY-Cut) y neuronal (DocLayout-YOLO).
/// Si el `OcrEnginePort` activo declara `provides_layout = true`, esta fase se omite.
pub trait LayoutEnginePort: Send + Sync {
    /// Retorna bloques con coordenadas y tipo; su orden define el orden de lectura.
    fn analyze(&self, page: &Page) -> Result<Vec<Block>, LayoutError>;

    fn name(&self) -> &str;
}

/// Motor de reconocimiento de texto sobre los bloques detectados por el layout.
pub trait OcrEnginePort: Send + Sync {
    /// Rellena `content` de cada bloque. El documento llega con estructura ya definida.
    fn process(&self, document: &mut Document, profile: &ProcessingProfile)
        -> Result<(), OcrError>;

    fn name(&self) -> &str;

    /// `true` si el motor combina deteccion de layout y OCR internamente (ej: docTR).
    ///
    /// Permite al pipeline saltar la fase de layout externo para evitar doble proceso.
    fn provides_layout(&self) -> bool { false }
}

/// Parsea un archivo fuente (PDF, PNG, JPEG) a un `Document` estructurado.
pub trait DocumentParserPort: Send + Sync {
    fn parse(&self, path: &Path) -> Result<Document, DocumentError>;
}

/// Persistencia de trabajos. Todas las operaciones son sincronas y duraderas.
pub trait JobStorePort: Send + Sync {
    fn save(&self, job: &Job) -> Result<(), JobStoreError>;
    fn get(&self, id: &str) -> Result<Job, JobStoreError>;
    /// Sobreescribe el estado usando `job.id` como clave de busqueda.
    fn update(&self, job: &Job) -> Result<(), JobStoreError>;
    fn list(&self) -> Result<Vec<Job>, JobStoreError>;
    fn delete(&self, id: &str) -> Result<(), JobStoreError>;
}

/// Serializa un `Job` completado al formato de salida (MD, JSON, PDF sandwich).
pub trait ExporterPort: Send + Sync {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError>;
    fn format_name(&self) -> &str;
}

/// Transformaciones de imagen previas al OCR (binarizacion, deskew, denoise).
///
/// Fase opcional: mejora CER en documentos escaneados pero puede perjudicar PDFs nativos.
pub trait PreprocessorPort: Send + Sync {
    fn preprocess(&self, document: &mut Document) -> Result<(), PreprocessError>;
}

/// Correcciones post-OCR sobre el texto de cada bloque (unicode, diccionario, espacios).
pub trait PostprocessorPort: Send + Sync {
    fn postprocess(&self, document: &mut Document) -> Result<(), OcrError>;
}

/// Analiza la estructura interna de bloques tipo `Table` con Table Transformer.
pub trait TableAnalyzerPort: Send + Sync {
    fn analyze_tables(&self, document: &mut Document) -> Result<(), LayoutError>;
    fn name(&self) -> &str;
}

/// Gestiona el ciclo de vida de modelos ONNX: descarga, verificacion SHA256 y localizacion.
pub trait ModelManagerPort: Send + Sync {
    /// Garantiza disponibilidad de todos los modelos; re-descarga si el checksum falla.
    fn ensure_models(&self) -> Result<std::path::PathBuf, ModelDownloadError>;
    fn model_exists(&self, model_name: &str) -> bool;
}
