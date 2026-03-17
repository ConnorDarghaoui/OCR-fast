//! Modulo de motores OCR basados en modelos ONNX.
//!
//! Este modulo contiene la implementacion del pipeline OCR completo
//! usando modelos ONNX para: orientacion, layout, deteccion de texto,
//! reconocimiento de texto y analisis de tablas.
//!
//! # Componentes
//!
//! - [`engine::OnnxOcrEngine`]: Motor unificado que orquesta todo el pipeline.
//! - [`model_downloader::ModelDownloader`]: Descarga automatica de modelos.
//! - [`preprocessing`]: Funciones de normalizacion por modelo.
//! - [`ctc_decoder`]: Decodificador CTC para PaddleOCR.
//! - [`layout::DocLayoutYoloEngine`]: Deteccion de layout con YOLO.
//! - [`orientation::OrientationDetector`]: Deteccion de orientacion.
//! - [`text_detection::TextDetector`]: Deteccion de texto con PaddleOCR.
//! - [`text_recognition::TextRecognizer`]: Reconocimiento de texto.
//! - [`table_analyzer::TableAnalyzer`]: Analisis de estructura de tablas.

pub mod ctc_decoder;
pub mod engine;
pub mod gpu_config;
pub mod layout;
pub mod model_downloader;
pub mod orientation;
pub mod preprocessing;
pub mod table_analyzer;
pub mod text_detection;
pub mod text_recognition;

pub use engine::OnnxOcrEngine;
pub use gpu_config::{inicializar as inicializar_gpu, EstadoGpu};
pub use model_downloader::ModelDownloader;
