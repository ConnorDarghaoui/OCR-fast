use std::path::PathBuf;
use thiserror::Error;

/// Errores recuperables durante ingesta y decodificación de documentos.
///
/// Se mantiene separado del resto del árbol para que la capa de aplicación pueda
/// distinguir fallos de input del usuario frente a errores de OCR o persistencia.
///
/// # Trade-offs
///
/// El enum privilegia clasificación operacional clara sobre granularidad extrema.
/// Detalles de librerías subyacentes se encapsulan en `String` para no filtrar
/// dependencias externas al dominio.
#[derive(Error, Debug)]
pub enum DocumentError {
    /// El archivo no existe en la ruta especificada.
    #[error("archivo no encontrado: {0}")]
    NotFound(PathBuf),

    /// El formato del archivo no esta soportado.
    #[error("formato no soportado: {0}")]
    UnsupportedFormat(String),

    /// Error al leer el contenido del archivo.
    #[error("error de lectura: {0}")]
    ReadError(#[from] std::io::Error),

    /// Error al procesar un PDF.
    #[error("error procesando PDF: {0}")]
    PdfError(String),

    /// Error al procesar una imagen.
    #[error("error procesando imagen: {0}")]
    ImageError(String),
}

/// Errores asociados a inicialización y ejecución del engine OCR.
///
/// Este enum separa indisponibilidad temporal (`NotReady`) de fallo estructural
/// (`NotInitialized` o `ModelLoadError`) para que la UI pueda decidir entre
/// reintento, backoff o degradación funcional sin inspeccionar mensajes libres.
#[derive(Error, Debug)]
pub enum OcrError {
    /// El motor OCR no esta inicializado correctamente.
    #[error("motor no inicializado: {0}")]
    NotInitialized(String),

    /// El motor OCR todavia esta cargando en segundo plano.
    ///
    /// Indica que el usuario inicio un job antes de que el motor ONNX
    /// terminase de inicializarse. El cliente debe reintentar una vez
    /// que el motor este disponible.
    #[error("motor OCR no listo: {0}")]
    NotReady(String),

    /// Error durante el proceso de reconocimiento.
    #[error("error de reconocimiento: {0}")]
    RecognitionError(String),

    /// Idioma no soportado por el motor.
    #[error("idioma no soportado: {0}")]
    UnsupportedLanguage(String),

    /// Error al cargar modelo ONNX.
    #[error("error cargando modelo: {0}")]
    ModelLoadError(String),
}

/// Errores producidos por segmentación geométrica y análisis de layout.
#[derive(Error, Debug)]
pub enum LayoutError {
    /// Error durante la segmentacion de bloques.
    #[error("error de segmentacion: {0}")]
    SegmentationError(String),

    /// La imagen de entrada no es valida para analisis.
    #[error("imagen invalida para layout: {0}")]
    InvalidImage(String),
}

/// Errores producidos al materializar un resultado OCR.
///
/// Exportación se modela por separado porque su semántica operacional es distinta:
/// un fallo aquí no invalida el OCR ya calculado, solo su persistencia final.
#[derive(Error, Debug)]
pub enum ExportError {
    /// Error al escribir el archivo de salida.
    #[error("error de escritura: {0}")]
    WriteError(#[from] std::io::Error),

    /// Error al serializar datos.
    #[error("error de serializacion: {0}")]
    SerializationError(String),

    /// Formato de exportacion no soportado.
    #[error("formato de exportacion no soportado: {0}")]
    UnsupportedFormat(String),
}

/// Errores de persistencia y coordinación en el almacén de trabajos.
///
/// La presencia de `LockError` visibiliza que el storage puede operar en contextos
/// concurrentes y evita colapsar errores lógicos y de sincronización en una misma
/// categoría opaca.
#[derive(Error, Debug)]
pub enum JobStoreError {
    /// El trabajo no fue encontrado.
    #[error("trabajo no encontrado: {0}")]
    NotFound(String),

    /// Error de concurrencia al acceder al almacen.
    #[error("error de concurrencia: {0}")]
    LockError(String),

    /// Error de persistencia.
    #[error("error de persistencia: {0}")]
    PersistenceError(String),
}

/// Errores de transformaciones previas al OCR sobre imágenes raster.
#[derive(Error, Debug)]
pub enum PreprocessError {
    /// Error durante binarizacion.
    #[error("error de binarizacion: {0}")]
    BinarizationError(String),

    /// Error durante correccion de inclinacion.
    #[error("error de deskew: {0}")]
    DeskewError(String),

    /// Error durante eliminacion de ruido.
    #[error("error de denoise: {0}")]
    DenoiseError(String),
}

/// Errores de adquisición, validación y localización de modelos ONNX.
///
/// La capa de modelos es una frontera de I/O, red e integridad, por lo que
/// necesita distinguir fallos transitorios (`NetworkError`) de corrupción dura
/// (`IntegrityError`) y de misconfiguración local (`DirectoryError`).
#[derive(Error, Debug)]
pub enum ModelDownloadError {
    /// Error de red al descargar un modelo.
    #[error("error de descarga: {0}")]
    NetworkError(String),

    /// Error al escribir el modelo en disco.
    #[error("error de escritura: {0}")]
    IoError(#[from] std::io::Error),

    /// Modelo no encontrado en el servidor remoto.
    #[error("modelo no encontrado: {0}")]
    NotFound(String),

    /// Error de integridad (checksum no coincide).
    #[error("error de integridad: esperado {expected}, obtenido {actual}")]
    IntegrityError { expected: String, actual: String },

    /// Directorio de modelos no accesible.
    #[error("directorio de modelos no accesible: {0}")]
    DirectoryError(String),
}

/// Errores emitidos por el subpipeline de orientación de documento.
#[derive(Error, Debug)]
pub enum OrientationError {
    /// Error al detectar la orientacion.
    #[error("error de deteccion de orientacion: {0}")]
    DetectionError(String),

    /// Imagen no valida para analisis de orientacion.
    #[error("imagen invalida para orientacion: {0}")]
    InvalidImage(String),
}

/// Error raíz del crate para composición entre capas.
///
/// `OcrFastError` unifica fronteras de fallo sin sacrificar tipado específico en
/// cada submódulo. La estrategia basada en `From` permite propagación idiomática
/// con `?` y mantiene observabilidad estructurada al nivel de capa.
///
/// # Trade-offs
///
/// Un error raíz reduce fricción en APIs de alto nivel, pero puede esconder
/// matices si el caller no hace matching explícito sobre sus variantes.
#[derive(Error, Debug)]
pub enum OcrFastError {
    #[error(transparent)]
    Document(#[from] DocumentError),

    #[error(transparent)]
    Ocr(#[from] OcrError),

    #[error(transparent)]
    Layout(#[from] LayoutError),

    #[error(transparent)]
    Export(#[from] ExportError),

    #[error(transparent)]
    JobStore(#[from] JobStoreError),

    #[error(transparent)]
    Preprocess(#[from] PreprocessError),

    #[error(transparent)]
    ModelDownload(#[from] ModelDownloadError),

    #[error(transparent)]
    Orientation(#[from] OrientationError),
}

/// Alias estándar para resultados públicos del crate.
///
/// # Notes
///
/// El alias reduce ruido en firmas de alto nivel y hace explícito que el crate
/// expone un error raíz estable para integración externa.
pub type Result<T> = std::result::Result<T, OcrFastError>;
