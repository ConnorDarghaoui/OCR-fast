/// Utilidades CTC para decodificar secuencias de reconocimiento.
pub mod ctc_decoder;
/// Motor OCR ONNX unificado de alto nivel.
pub mod engine;
/// Selección y configuración de execution providers GPU/CPU.
pub mod gpu_config;
/// Detector de layout basado en DocLayout-YOLO.
pub mod layout;
/// Gestión local de artefactos de modelos ONNX.
pub mod model_downloader;
/// Detección de orientación con PP-LCNet.
pub mod orientation;
/// Normalización de tensores por modelo.
pub mod preprocessing;
/// Aprovisionamiento de runtime y modelos previo a la carga del engine.
pub mod runtime_provisioner;
/// Reconstrucción de tablas con Table Transformer.
pub mod table_analyzer;
/// Detección de líneas/regiones de texto.
pub mod text_detection;
/// Reconocimiento textual por línea y batch.
pub mod text_recognition;

/// Reexporta el engine ONNX listo para integración.
pub use engine::OnnxOcrEngine;
/// Reexporta el bootstrap de GPU y su estado observable.
pub use gpu_config::{inicializar as inicializar_gpu, EstadoGpu};
/// Reexporta el gestor de modelos ONNX.
pub use model_downloader::ModelDownloader;
/// Reexporta el provisioner de runtime ONNX y su resultado materializado.
pub use runtime_provisioner::{ModelRuntimeProvisioner, ProvisionedOnnxRuntime};
