use crate::domain::errors::LayoutError;
use crate::domain::{Block, BlockType, Rectangle};
use crate::infrastructure::ocr_engines::onnx::preprocessing;
use image::DynamicImage;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

/// Detector de layout semántico basado en DocLayout-YOLO.
///
/// Este adaptador captura casos donde un enfoque heurístico como XY-Cut pierde
/// demasiada precisión. Se usa `Mutex<Session>` por las mismas razones que el
/// resto del stack ONNX: compartir sesiones sin ownership exclusivo en workers.
pub struct DocLayoutYoloEngine {
    /// Sesion ONNX del modelo (Mutex por &mut self en run).
    sesion: Mutex<Session>,
    /// Umbral de confianza para filtrar detecciones.
    umbral_confianza: f32,
    /// Umbral de IoU para Non-Maximum Suppression.
    umbral_nms: f32,
}

impl DocLayoutYoloEngine {
    /// Carga el modelo DocLayout-YOLO y prepara la sesión de inferencia.
    pub fn new(ruta_modelo: &Path) -> Result<Self, LayoutError> {
        let sesion = Session::builder()
            .and_then(|b| b.with_intra_threads(4))
            .and_then(|b| b.with_execution_providers(super::gpu_config::providers(0)))
            .and_then(|b| b.commit_from_file(ruta_modelo))
            .map_err(|e| {
                LayoutError::SegmentationError(format!(
                    "Error cargando modelo DocLayout-YOLO: {}",
                    e
                ))
            })?;

        log::info!("DocLayout-YOLO cargado desde: {:?}", ruta_modelo);

        Ok(Self {
            sesion: Mutex::new(sesion),
            umbral_confianza: 0.3,
            umbral_nms: 0.45,
        })
    }

    /// Analiza una imagen y retorna bloques usando los umbrales por defecto.
    pub fn analizar_imagen(&self, imagen: &DynamicImage) -> Result<Vec<Block>, LayoutError> {
        self.analizar_imagen_con_umbrales(imagen, self.umbral_confianza, self.umbral_nms)
    }

    /// Analiza una imagen con umbrales explícitos para ajuste por perfil.
    pub fn analizar_imagen_con_umbrales(
        &self,
        imagen: &DynamicImage,
        umbral_confianza: f32,
        umbral_nms: f32,
    ) -> Result<Vec<Block>, LayoutError> {
        let (tensor_datos, escala_inv_x, escala_inv_y) =
            preprocessing::preparar_para_layout(imagen);

        let forma = tensor_datos.shape().to_vec();
        let datos_flat: Vec<f32> = tensor_datos.as_standard_layout().iter().cloned().collect();
        let forma_i64: Vec<i64> = forma.iter().map(|&d| d as i64).collect();

        let input_tensor = Tensor::from_array((forma_i64, datos_flat))
            .map_err(|e| LayoutError::SegmentationError(format!("Error creando tensor: {}", e)))?;

        let mut sesion = self
            .sesion
            .lock()
            .map_err(|e| LayoutError::SegmentationError(format!("Mutex poisoned: {}", e)))?;
        let salidas = sesion.run(ort::inputs![input_tensor]).map_err(|e| {
            LayoutError::SegmentationError(format!("Error ejecutando inferencia: {}", e))
        })?;

        let detecciones_raw = self.parsear_salida_yolo_con_umbral(&salidas, umbral_confianza)?;
        let detecciones_filtradas = aplicar_nms(detecciones_raw, umbral_nms);
        let mut bloques: Vec<Block> = Vec::new();

        for (i, det) in detecciones_filtradas.iter().enumerate() {
            let x = (det.x * escala_inv_x) as u32;
            let y = (det.y * escala_inv_y) as u32;
            let ancho = (det.ancho * escala_inv_x) as u32;
            let alto = (det.alto * escala_inv_y) as u32;

            let tipo_bloque = match det.clase {
                0 => BlockType::Title,
                1 => BlockType::Text,
                2 => continue,
                3 | 4 => BlockType::Image,
                5 | 6 | 7 => BlockType::Table,
                8 | 9 => BlockType::Formula,
                _ => BlockType::Unknown,
            };

            bloques.push(Block {
                block_type: tipo_bloque,
                bounding_box: Rectangle {
                    x,
                    y,
                    width: ancho,
                    height: alto,
                },
                content: String::new(),
                confidence: det.confianza as f64,
                embedded_image: None,
                table_structure: None,
                reading_order: i as u32,
            });
        }

        bloques.sort_by_key(|b| b.bounding_box.y);
        for (i, bloque) in bloques.iter_mut().enumerate() {
            bloque.reading_order = i as u32;
        }

        log::info!("DocLayout-YOLO detecto {} bloques", bloques.len());
        Ok(bloques)
    }

    /// Parsea la salida raw del modelo YOLO con umbral de confianza explicito.
    /// Formato: [1, 4+num_clases, num_detecciones]
    fn parsear_salida_yolo_con_umbral(
        &self,
        salidas: &ort::session::SessionOutputs,
        umbral_confianza: f32,
    ) -> Result<Vec<DeteccionRaw>, LayoutError> {
        let (forma_info, datos) = salidas[0].try_extract_tensor::<f32>().map_err(|e| {
            LayoutError::SegmentationError(format!("Error extrayendo tensor: {}", e))
        })?;

        let forma = &*forma_info;

        if forma.len() != 3 {
            return Err(LayoutError::SegmentationError(format!(
                "Forma inesperada del output: {:?}",
                forma
            )));
        }

        let num_atributos = forma[1] as usize;
        let num_detecciones = forma[2] as usize;
        let num_clases = num_atributos - 4;

        let mut detecciones = Vec::new();

        for d in 0..num_detecciones {
            let cx = datos[0 * num_detecciones + d];
            let cy = datos[1 * num_detecciones + d];
            let w = datos[2 * num_detecciones + d];
            let h = datos[3 * num_detecciones + d];

            let mut mejor_clase = 0usize;
            let mut mejor_score = f32::NEG_INFINITY;

            for c in 0..num_clases {
                let score = datos[(4 + c) * num_detecciones + d];
                if score > mejor_score {
                    mejor_score = score;
                    mejor_clase = c;
                }
            }

            if mejor_score < umbral_confianza {
                continue;
            }

            detecciones.push(DeteccionRaw {
                x: cx - w / 2.0,
                y: cy - h / 2.0,
                ancho: w,
                alto: h,
                confianza: mejor_score,
                clase: mejor_clase,
            });
        }

        Ok(detecciones)
    }
}

/// Aplica Non-Maximum Suppression para eliminar detecciones duplicadas.
///
/// Funcion libre para facilitar testing unitario sin instancia del engine.
fn aplicar_nms(mut detecciones: Vec<DeteccionRaw>, umbral_nms: f32) -> Vec<DeteccionRaw> {
    detecciones.sort_by(|a, b| {
        b.confianza
            .partial_cmp(&a.confianza)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut resultado: Vec<DeteccionRaw> = Vec::new();

    for det in &detecciones {
        let mut suprimir = false;
        for existente in &resultado {
            if det.clase == existente.clase && calcular_iou(det, existente) > umbral_nms {
                suprimir = true;
                break;
            }
        }
        if !suprimir {
            resultado.push(det.clone());
        }
    }

    resultado
}

#[derive(Debug, Clone)]
struct DeteccionRaw {
    x: f32,
    y: f32,
    ancho: f32,
    alto: f32,
    confianza: f32,
    clase: usize,
}

fn calcular_iou(a: &DeteccionRaw, b: &DeteccionRaw) -> f32 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.ancho).min(b.x + b.ancho);
    let y2 = (a.y + a.alto).min(b.y + b.alto);

    let interseccion = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = a.ancho * a.alto;
    let area_b = b.ancho * b.alto;
    let union = area_a + area_b - interseccion;

    if union > 0.0 {
        interseccion / union
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crear_deteccion(
        x: f32,
        y: f32,
        ancho: f32,
        alto: f32,
        confianza: f32,
        clase: usize,
    ) -> DeteccionRaw {
        DeteccionRaw {
            x,
            y,
            ancho,
            alto,
            confianza,
            clase,
        }
    }

    #[test]
    fn test_iou_cajas_identicas() {
        let a = crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.9, 0);
        let b = crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.8, 0);
        let iou = calcular_iou(&a, &b);
        assert!(
            (iou - 1.0).abs() < 0.001,
            "IoU de cajas identicas debe ser 1.0, fue: {}",
            iou
        );
    }

    #[test]
    fn test_iou_sin_overlap() {
        let a = crear_deteccion(0.0, 0.0, 50.0, 50.0, 0.9, 0);
        let b = crear_deteccion(100.0, 100.0, 50.0, 50.0, 0.8, 0);
        let iou = calcular_iou(&a, &b);
        assert!(
            iou.abs() < 0.001,
            "IoU sin overlap debe ser 0.0, fue: {}",
            iou
        );
    }

    #[test]
    fn test_iou_overlap_parcial() {
        // Caja A: [0,0] a [100,100], area = 10000
        // Caja B: [50,50] a [150,150], area = 10000
        // Interseccion: [50,50] a [100,100] = 50*50 = 2500
        // Union: 10000 + 10000 - 2500 = 17500
        // IoU = 2500/17500 ~ 0.1428
        let a = crear_deteccion(0.0, 0.0, 100.0, 100.0, 0.9, 0);
        let b = crear_deteccion(50.0, 50.0, 100.0, 100.0, 0.8, 0);
        let iou = calcular_iou(&a, &b);
        assert!(
            (iou - 0.1428).abs() < 0.01,
            "IoU parcial incorrecto: {}",
            iou
        );
    }

    #[test]
    fn test_nms_elimina_duplicados() {
        let detecciones = vec![
            // Deteccion alta confianza
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.95, 1),
            // Casi identica, menor confianza -> debe suprimirse (IoU 1.0 > 0.45)
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.80, 1),
            // Diferente ubicacion, misma clase -> debe mantenerse
            crear_deteccion(300.0, 300.0, 100.0, 100.0, 0.70, 1),
            // Misma ubicacion pero diferente clase -> debe mantenerse
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.85, 2),
        ];

        let resultado = aplicar_nms(detecciones, 0.45);

        assert_eq!(resultado.len(), 3, "NMS debe eliminar 1 de 4 detecciones");
        // La de mayor confianza (0.95) debe ser la primera
        assert!((resultado[0].confianza - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_nms_sin_supresion_cuando_clases_distintas() {
        let detecciones = vec![
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.9, 0),
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.8, 1),
            crear_deteccion(10.0, 10.0, 100.0, 100.0, 0.7, 2),
        ];

        let resultado = aplicar_nms(detecciones, 0.45);
        assert_eq!(
            resultado.len(),
            3,
            "NMS no debe suprimir detecciones de clases distintas"
        );
    }

    #[test]
    fn test_iou_caja_contenida() {
        // B completamente dentro de A
        // A: [0,0] a [200,200], area = 40000
        // B: [50,50] a [100,100], area = 2500
        // Interseccion = 2500
        // Union = 40000 + 2500 - 2500 = 40000
        // IoU = 2500/40000 = 0.0625
        let a = crear_deteccion(0.0, 0.0, 200.0, 200.0, 0.9, 0);
        let b = crear_deteccion(50.0, 50.0, 50.0, 50.0, 0.8, 0);
        let iou = calcular_iou(&a, &b);
        assert!(
            (iou - 0.0625).abs() < 0.01,
            "IoU contenida incorrecto: {}",
            iou
        );
    }
}
