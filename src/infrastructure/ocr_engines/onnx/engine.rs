use crate::domain::errors::OcrError;
use crate::domain::{Block, BlockType, Document, ProcessingProfile, Rectangle};
use crate::infrastructure::ocr_engines::onnx::layout::DocLayoutYoloEngine;
use crate::infrastructure::ocr_engines::onnx::orientation::OrientationDetector;
use crate::infrastructure::ocr_engines::onnx::runtime_provisioner::{
    ModelRuntimeProvisioner, ProvisionedOnnxRuntime,
};
use crate::infrastructure::ocr_engines::onnx::table_analyzer::TableAnalyzer;
use crate::infrastructure::ocr_engines::onnx::text_detection::TextDetector;
use crate::infrastructure::ocr_engines::onnx::text_recognition::TextRecognizer;
use crate::interfaces::ports::OcrEnginePort;
use image::{DynamicImage, GenericImageView};
use std::path::Path;

/// Engine OCR multi-etapa respaldado por un conjunto coordinado de modelos ONNX.
///
/// La implementación integra orientación, layout, detección de texto,
/// reconocimiento y tablas bajo una sola fachada del puerto `OcrEnginePort`.
/// Eso reduce acoplamiento en la capa de aplicación y permite optimizar la
/// comunicación entre submodelos sin exponer sus detalles a la TUI.
///
/// # Concurrency
///
/// Cada subengine protege su `Session` con `Mutex` porque ONNX Runtime se usa a
/// través de referencias compartidas en workers. La elección favorece seguridad y
/// simplicidad sobre paralelismo fino dentro de una misma instancia.
pub struct OnnxOcrEngine {
    orientacion: OrientationDetector,
    layout: DocLayoutYoloEngine,
    texto_det: TextDetector,
    texto_rec: TextRecognizer,
    tablas: TableAnalyzer,
}

impl OnnxOcrEngine {
    /// Construye el engine ONNX completo resolviendo GPU y modelos requeridos.
    ///
    /// # Errors
    ///
    /// Falla si la adquisición de modelos o la carga de cualquiera de los
    /// submodelos no puede completarse de forma consistente.
    pub fn new() -> Result<Self, OcrError> {
        let runtime = ModelRuntimeProvisioner::new()
            .and_then(|provisioner| provisioner.provision(None, None, None))
            .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        Self::from_provisioned_runtime(&runtime)
    }

    /// Construye el engine a partir de un runtime ya aprovisionado.
    ///
    /// # Trade-offs
    ///
    /// Esta variante mantiene la inferencia desacoplada de red y filesystem. El
    /// caller controla cuándo y cómo aprovisionar modelos sin que el engine
    /// replique esa política dentro de su constructor.
    pub fn from_provisioned_runtime(runtime: &ProvisionedOnnxRuntime) -> Result<Self, OcrError> {
        Self::from_directory(runtime.ruta_modelos())
    }

    /// Construye el engine a partir de un directorio de modelos ya materializado.
    ///
    /// # Trade-offs
    ///
    /// Recibir una ruta explícita separa bootstrap de artefactos de bootstrap de
    /// sesiones, lo que facilita tests y empaquetado offline.
    pub fn from_directory(ruta_modelos: &Path) -> Result<Self, OcrError> {
        log::info!("Cargando modelos ONNX desde: {:?}", ruta_modelos);

        let orientacion =
            OrientationDetector::new(&ruta_modelos.join("orientation/PP-LCNet_x1_0_doc_ori.onnx"))
                .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        let layout = DocLayoutYoloEngine::new(
            &ruta_modelos.join("layout/doclayout_yolo_docstructbench_imgsz1024.onnx"),
        )
        .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        let texto_det = TextDetector::new(&ruta_modelos.join("ocr/det.onnx"))
            .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        let texto_rec = TextRecognizer::new(
            &ruta_modelos.join("ocr/rec.onnx"),
            &ruta_modelos.join("ocr/dict.txt"),
        )
        .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        let tablas = TableAnalyzer::new(&ruta_modelos.join("table/model_uint8.onnx"))
            .map_err(|e| OcrError::ModelLoadError(e.to_string()))?;

        log::info!("Todos los modelos ONNX cargados exitosamente");

        Ok(Self {
            orientacion,
            layout,
            texto_det,
            texto_rec,
            tablas,
        })
    }

    /// Procesa una pagina individual aplicando el perfil de procesamiento.
    fn procesar_pagina(
        &self,
        imagen: &DynamicImage,
        profile: &ProcessingProfile,
    ) -> Result<Vec<Block>, OcrError> {
        let (umbral_conf, umbral_nms, umbral_bin, tam_min): (f32, f32, f32, u32) = match profile {
            ProcessingProfile::Fast => (0.25, 0.50, 0.35, 8),
            ProcessingProfile::Balanced => (0.30, 0.45, 0.30, 5),
            ProcessingProfile::Accurate => (0.40, 0.35, 0.25, 3),
        };

        let imagen_corregida = match profile {
            ProcessingProfile::Fast => {
                log::debug!("Fast profile: omitiendo correccion de orientacion");
                imagen.clone()
            }
            _ => self
                .orientacion
                .corregir(imagen)
                .map_err(|e| OcrError::RecognitionError(e.to_string()))?,
        };

        let mut bloques = self
            .layout
            .analizar_imagen_con_umbrales(&imagen_corregida, umbral_conf, umbral_nms)
            .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

        for bloque in &mut bloques {
            match bloque.block_type {
                BlockType::Text | BlockType::Title | BlockType::List => {
                    self.procesar_bloque_texto(&imagen_corregida, bloque, umbral_bin, tam_min);
                }
                BlockType::Table => {
                    if *profile != ProcessingProfile::Fast {
                        self.procesar_bloque_tabla(&imagen_corregida, bloque);
                    }
                }
                BlockType::Image | BlockType::Signature | BlockType::Stamp => {
                    self.extraer_imagen_embebida(&imagen_corregida, bloque);
                }
                _ => {}
            }
        }

        Ok(bloques)
    }

    /// Detecta lineas y reconoce texto dentro de un bloque usando inferencia batch.
    ///
    /// Todos los recortes de linea se procesan en una sola llamada al modelo de
    /// reconocimiento, reduciendo el overhead de despacho especialmente en GPU.
    fn procesar_bloque_texto(
        &self,
        imagen: &DynamicImage,
        bloque: &mut Block,
        umbral_bin: f32,
        tam_min: u32,
    ) {
        let recorte = match self.recortar_region(imagen, &bloque.bounding_box) {
            Some(img) => img,
            None => return,
        };

        let regiones = self
            .texto_det
            .detectar_con_umbrales(&recorte, umbral_bin, tam_min)
            .unwrap_or_default();

        let recortes_linea: Vec<DynamicImage> = if regiones.is_empty() {
            vec![recorte]
        } else {
            regiones
                .iter()
                .filter_map(|r| self.recortar_region(&recorte, r))
                .collect()
        };

        if recortes_linea.is_empty() {
            return;
        }

        let resultados = match self.texto_rec.reconocer_batch(&recortes_linea) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Error en reconocimiento batch: {}", e);
                return;
            }
        };

        let mut lineas: Vec<String> = Vec::new();
        let mut confianza_total = 0.0f64;
        let mut conteo = 0u32;

        for res in resultados {
            if !res.texto.is_empty() {
                lineas.push(res.texto);
                confianza_total += res.confianza;
                conteo += 1;
            }
        }

        bloque.content = lineas.join("\n");
        bloque.confidence = if conteo > 0 {
            confianza_total / conteo as f64
        } else {
            0.0
        };
    }

    /// Analiza estructura y contenido de una tabla.
    fn procesar_bloque_tabla(&self, imagen: &DynamicImage, bloque: &mut Block) {
        let recorte = match self.recortar_region(imagen, &bloque.bounding_box) {
            Some(img) => img,
            None => return,
        };

        match self.tablas.analizar(&recorte) {
            Ok(mut estructura) => {
                for fila in &mut estructura.rows {
                    for celda in fila {
                        if let Some(celda_img) = self.recortar_region(&recorte, &celda.bounding_box)
                        {
                            if let Ok(res) = self.texto_rec.reconocer(&celda_img) {
                                celda.content = res.texto;
                            }
                        }
                    }
                }

                bloque.content = estructura.to_markdown();
                bloque.table_structure = Some(estructura);
            }
            Err(e) => log::warn!("Error analizando tabla: {}", e),
        }
    }

    /// Extrae la imagen embebida de un bloque.
    fn extraer_imagen_embebida(&self, imagen: &DynamicImage, bloque: &mut Block) {
        if let Some(recorte) = self.recortar_region(imagen, &bloque.bounding_box) {
            let mut buffer = std::io::Cursor::new(Vec::new());
            if recorte
                .write_to(&mut buffer, image::ImageFormat::Png)
                .is_ok()
            {
                bloque.embedded_image = Some(buffer.into_inner());
            }
        }
    }

    /// Recorta una region rectangular de una imagen con validacion de limites.
    fn recortar_region(&self, imagen: &DynamicImage, rect: &Rectangle) -> Option<DynamicImage> {
        let (ancho_img, alto_img) = imagen.dimensions();

        if rect.x >= ancho_img || rect.y >= alto_img {
            return None;
        }

        let x = rect.x.min(ancho_img - 1);
        let y = rect.y.min(alto_img - 1);
        let ancho = rect.width.min(ancho_img - x);
        let alto = rect.height.min(alto_img - y);

        if ancho < 2 || alto < 2 {
            return None;
        }

        Some(imagen.crop_imm(x, y, ancho, alto))
    }
}

impl OcrEnginePort for OnnxOcrEngine {
    fn process(
        &self,
        document: &mut Document,
        profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        log::info!(
            "OnnxOcrEngine: procesando {} paginas con perfil {:?}",
            document.pages.len(),
            profile
        );

        for pagina in &mut document.pages {
            let imagen = match &pagina.image_data {
                Some(bytes) => image::load_from_memory(bytes)
                    .map_err(|e| OcrError::RecognitionError(format!("Error imagen: {}", e)))?,
                None => {
                    log::warn!("Pagina {} sin imagen, omitiendo", pagina.number);
                    continue;
                }
            };

            let bloques = self.procesar_pagina(&imagen, profile)?;
            pagina.blocks = bloques;
            log::info!("Pagina {}: {} bloques", pagina.number, pagina.blocks.len());
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "OnnxOcrEngine"
    }

    fn provides_layout(&self) -> bool {
        true
    }
}
