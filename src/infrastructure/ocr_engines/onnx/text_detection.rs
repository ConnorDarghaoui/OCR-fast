//! Deteccion de texto usando PaddleOCR v5 (modelo DB).
//!
//! Detecta regiones de texto en una imagen y retorna bounding boxes
//! de lineas de texto individuales.

use crate::domain::Rectangle;
use crate::infrastructure::ocr_engines::onnx::preprocessing;
use image::{DynamicImage, GenericImageView};
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

/// Umbral de binarizacion del mapa de probabilidad del modelo DB.
/// Pixeles con score mayor a este valor se consideran texto.
const UMBRAL_BINARIZACION_DEFAULT: f32 = 0.3;

/// Tamano minimo en pixeles (ancho y alto) de una caja de texto valida.
/// Cajas menores se descartan como artefactos del modelo.
const TAMANO_MINIMO_CAJA_DEFAULT: u32 = 5;

/// Altura de banda en pixeles para agrupar lineas durante el ordenamiento.
/// Dos cajas cuyo `y` cae en el mismo multiplo de esta constante se
/// consideran en la misma fila y se ordenan de izquierda a derecha.
const BANDA_AGRUPACION_FILAS: u32 = 20;

/// Detector de texto basado en PaddleOCR v5 DB.
pub struct TextDetector {
    sesion: Mutex<Session>,
    umbral_binarizacion: f32,
    tamano_minimo_caja: u32,
}

impl TextDetector {
    /// Carga el modelo PaddleOCR DB desde la ruta indicada.
    pub fn new(ruta_modelo: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let sesion = Session::builder()
            .and_then(|b| b.with_intra_threads(4))
            .and_then(|b| b.with_execution_providers(super::gpu_config::providers(0)))
            .and_then(|b| b.commit_from_file(ruta_modelo))
            .map_err(|e| format!("Error cargando PaddleOCR det: {}", e))?;

        log::info!("PaddleOCR Text Detection cargado desde: {:?}", ruta_modelo);

        Ok(Self {
            sesion: Mutex::new(sesion),
            umbral_binarizacion: UMBRAL_BINARIZACION_DEFAULT,
            tamano_minimo_caja: TAMANO_MINIMO_CAJA_DEFAULT,
        })
    }

    /// Detecta regiones de texto en una imagen.
    pub fn detectar(
        &self,
        imagen: &DynamicImage,
    ) -> Result<Vec<Rectangle>, Box<dyn std::error::Error + Send + Sync>> {
        self.detectar_con_umbrales(imagen, self.umbral_binarizacion, self.tamano_minimo_caja)
    }

    /// Detecta regiones de texto con umbrales explicitos para ajuste por perfil de procesamiento.
    pub fn detectar_con_umbrales(
        &self,
        imagen: &DynamicImage,
        umbral_binarizacion: f32,
        tamano_minimo: u32,
    ) -> Result<Vec<Rectangle>, Box<dyn std::error::Error + Send + Sync>> {
        let (ancho_original, alto_original) = imagen.dimensions();

        let tensor_datos = preprocessing::preparar_para_deteccion_texto(imagen);
        let forma = tensor_datos.shape().to_vec();
        let alto_input = forma[2] as u32;
        let ancho_input = forma[3] as u32;
        let forma_i64: Vec<i64> = forma.iter().map(|&d| d as i64).collect();
        let datos_flat: Vec<f32> = tensor_datos.as_standard_layout().iter().cloned().collect();

        let input_tensor = Tensor::from_array((forma_i64, datos_flat))?;

        let mut sesion = self.sesion.lock()
            .map_err(|e| format!("Mutex poisoned: {}", e))?;
        let salidas = sesion.run(ort::inputs![input_tensor])
            .map_err(|e| format!("Error inferencia: {}", e))?;

        let (_forma, datos) = salidas[0].try_extract_tensor::<f32>()?;

        let mapa_binario = self.binarizar_mapa(datos, ancho_input as usize, alto_input as usize, umbral_binarizacion);
        let cajas = self.encontrar_cajas(&mapa_binario, ancho_input, alto_input, tamano_minimo);

        let escala_x = ancho_original as f32 / ancho_input as f32;
        let escala_y = alto_original as f32 / alto_input as f32;

        let mut resultados: Vec<Rectangle> = cajas
            .into_iter()
            .map(|(x, y, w, h)| Rectangle {
                x: (x as f32 * escala_x) as u32,
                y: (y as f32 * escala_y) as u32,
                width: (w as f32 * escala_x) as u32,
                height: (h as f32 * escala_y) as u32,
            })
            .filter(|r| r.width >= tamano_minimo && r.height >= tamano_minimo)
            .collect();

        resultados.sort_by(|a, b| {
            let fila_a = a.y / BANDA_AGRUPACION_FILAS;
            let fila_b = b.y / BANDA_AGRUPACION_FILAS;
            if fila_a == fila_b { a.x.cmp(&b.x) } else { a.y.cmp(&b.y) }
        });

        log::info!("PaddleOCR detecto {} regiones de texto", resultados.len());
        Ok(resultados)
    }

    fn binarizar_mapa(&self, datos: &[f32], ancho: usize, alto: usize, umbral: f32) -> Vec<u8> {
        datos.iter()
            .take(ancho * alto)
            .map(|&v| if v > umbral { 255 } else { 0 })
            .collect()
    }

    fn encontrar_cajas(&self, mapa: &[u8], ancho: u32, alto: u32, tamano_minimo: u32) -> Vec<(u32, u32, u32, u32)> {
        let mut visitado = vec![false; (ancho * alto) as usize];
        let mut cajas = Vec::new();

        for y in 0..alto {
            for x in 0..ancho {
                let idx = (y * ancho + x) as usize;
                if mapa[idx] == 255 && !visitado[idx] {
                    let (min_x, min_y, max_x, max_y) =
                        self.flood_fill(mapa, &mut visitado, x, y, ancho, alto);

                    let w = max_x - min_x + 1;
                    let h = max_y - min_y + 1;

                    if w >= tamano_minimo && h >= tamano_minimo {
                        cajas.push((min_x, min_y, w, h));
                    }
                }
            }
        }

        cajas
    }

    fn flood_fill(
        &self, mapa: &[u8], visitado: &mut [bool],
        inicio_x: u32, inicio_y: u32, ancho: u32, alto: u32,
    ) -> (u32, u32, u32, u32) {
        let mut pila = vec![(inicio_x, inicio_y)];
        let (mut min_x, mut min_y) = (inicio_x, inicio_y);
        let (mut max_x, mut max_y) = (inicio_x, inicio_y);

        while let Some((x, y)) = pila.pop() {
            let idx = (y * ancho + x) as usize;
            if idx >= mapa.len() || visitado[idx] || mapa[idx] != 255 {
                continue;
            }

            visitado[idx] = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);

            if x > 0 { pila.push((x - 1, y)); }
            if x + 1 < ancho { pila.push((x + 1, y)); }
            if y > 0 { pila.push((x, y - 1)); }
            if y + 1 < alto { pila.push((x, y + 1)); }
        }

        (min_x, min_y, max_x, max_y)
    }
}
