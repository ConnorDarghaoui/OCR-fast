//! Modulo de errores tipados del sistema OCR
//!
//! Define los tipos de error para cada capa siguiendo un enfoque estructurado
//! que facilita el manejo y la propagacion de errores a traves del sistema.

use std::path::PathBuf;
use thiserror::Error;

/// Errores relacionados con la ingesta y parseo de documentos.
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

/// Errores relacionados con el motor OCR.
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

/// Errores relacionados con el analisis de layout.
#[derive(Error, Debug)]
pub enum LayoutError {
    /// Error durante la segmentacion de bloques.
    #[error("error de segmentacion: {0}")]
    SegmentationError(String),

    /// La imagen de entrada no es valida para analisis.
    #[error("imagen invalida para layout: {0}")]
    InvalidImage(String),
}

/// Errores relacionados con la exportacion de resultados.
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

/// Errores relacionados con el almacenamiento de trabajos.
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

/// Errores relacionados con preprocesamiento de imagenes.
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

/// Errores relacionados con descarga y gestion de modelos ONNX.
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
    IntegrityError {
        expected: String,
        actual: String,
    },

    /// Directorio de modelos no accesible.
    #[error("directorio de modelos no accesible: {0}")]
    DirectoryError(String),
}

/// Errores relacionados con deteccion de orientacion.
#[derive(Error, Debug)]
pub enum OrientationError {
    /// Error al detectar la orientacion.
    #[error("error de deteccion de orientacion: {0}")]
    DetectionError(String),

    /// Imagen no valida para analisis de orientacion.
    #[error("imagen invalida para orientacion: {0}")]
    InvalidImage(String),
}

/// Error unificado del sistema OCR.
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

/// Alias conveniente para resultados del sistema.
pub type Result<T> = std::result::Result<T, OcrFastError>;
