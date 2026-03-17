//! Deteccion de orientacion de documentos usando PP-LCNet.
//!
//! Detecta si una pagina esta rotada 0, 90, 180 o 270 grados
//! y proporciona la correccion necesaria.

use crate::domain::errors::OrientationError;
use crate::infrastructure::ocr_engines::onnx::preprocessing;
use image::DynamicImage;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

/// Angulos posibles de orientacion detectables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientacion {
    Normal,
    Rotado90,
    Rotado180,
    Rotado270,
}

/// Motor de deteccion de orientacion basado en PP-LCNet.
pub struct OrientationDetector {
    sesion: Mutex<Session>,
}

impl OrientationDetector {
    /// Carga el modelo PP-LCNet desde la ruta indicada.
    pub fn new(ruta_modelo: &Path) -> Result<Self, OrientationError> {
        let sesion = Session::builder()
            .and_then(|b| b.with_intra_threads(2))
            .and_then(|b| b.with_execution_providers(super::gpu_config::providers(0)))
            .and_then(|b| b.commit_from_file(ruta_modelo))
            .map_err(|e| OrientationError::DetectionError(
                format!("Error cargando PP-LCNet: {}", e)
            ))?;

        log::info!("PP-LCNet Orientation cargado desde: {:?}", ruta_modelo);

        Ok(Self { sesion: Mutex::new(sesion) })
    }

    /// Clasifica la orientacion de la imagen (0/90/180/270 grados).
    pub fn detectar(&self, imagen: &DynamicImage) -> Result<Orientacion, OrientationError> {
        let tensor_datos = preprocessing::preparar_para_orientacion(imagen);
        let forma: Vec<i64> = tensor_datos.shape().iter().map(|&d| d as i64).collect();
        let datos_flat: Vec<f32> = tensor_datos.as_standard_layout().iter().cloned().collect();

        let input_tensor = Tensor::from_array((forma, datos_flat))
            .map_err(|e| OrientationError::DetectionError(format!("Error tensor: {}", e)))?;

        let mut sesion = self.sesion.lock()
            .map_err(|e| OrientationError::DetectionError(format!("Mutex poisoned: {}", e)))?;
        let salidas = sesion.run(ort::inputs![input_tensor])
            .map_err(|e| OrientationError::DetectionError(format!("Error inferencia: {}", e)))?;

        let (_forma, datos) = salidas[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| OrientationError::DetectionError(format!("Error extraccion: {}", e)))?;

        let (clase, confianza) = datos
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));

        let orientacion = match clase {
            0 => Orientacion::Normal,
            1 => Orientacion::Rotado90,
            2 => Orientacion::Rotado180,
            3 => Orientacion::Rotado270,
            _ => Orientacion::Normal,
        };

        log::info!("Orientacion: {:?} (confianza: {:.2}%)", orientacion, confianza * 100.0);
        Ok(orientacion)
    }

    /// Detecta la orientacion y aplica la rotacion correctiva necesaria.
    pub fn corregir(&self, imagen: &DynamicImage) -> Result<DynamicImage, OrientationError> {
        let orientacion = self.detectar(imagen)?;
        let corregida = match orientacion {
            Orientacion::Normal => imagen.clone(),
            Orientacion::Rotado90 => imagen.rotate270(),
            Orientacion::Rotado180 => imagen.rotate180(),
            Orientacion::Rotado270 => imagen.rotate90(),
        };
        Ok(corregida)
    }
}
