use crate::infrastructure::ocr_engines::onnx::ctc_decoder;
use crate::infrastructure::ocr_engines::onnx::preprocessing;
use image::DynamicImage;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

/// Reconocedor de líneas de texto basado en PaddleOCR Latin.
///
/// La estructura encapsula la sesión ONNX y el diccionario CTC. Agrupar ambos en
/// una sola unidad evita inconsistencias entre modelo y vocabulario.
pub struct TextRecognizer {
    sesion: Mutex<Session>,
    diccionario: Vec<String>,
}

/// Resultado estable de reconocimiento para una línea individual.
///
/// Separar texto y confianza permite que callers decidan postproceso, filtrado o
/// UI sin reinterpretar el payload bruto de CTC.
#[derive(Debug, Clone)]
pub struct ResultadoReconocimiento {
    pub texto: String,
    pub confianza: f64,
}

impl TextRecognizer {
    /// Carga el modelo de reconocimiento y su diccionario asociado.
    pub fn new(
        ruta_modelo: &Path,
        ruta_diccionario: &Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let sesion = Session::builder()
            .and_then(|b| b.with_intra_threads(2))
            .and_then(|b| b.with_execution_providers(super::gpu_config::providers(0)))
            .and_then(|b| b.commit_from_file(ruta_modelo))
            .map_err(|e| format!("Error cargando PaddleOCR rec: {}", e))?;

        let diccionario = ctc_decoder::cargar_diccionario(ruta_diccionario)?;

        log::info!(
            "PaddleOCR Text Recognition cargado ({} caracteres)",
            diccionario.len()
        );

        Ok(Self {
            sesion: Mutex::new(sesion),
            diccionario,
        })
    }

    /// Reconoce texto en un recorte correspondiente a una sola línea.
    pub fn reconocer(
        &self,
        recorte: &DynamicImage,
    ) -> Result<ResultadoReconocimiento, Box<dyn std::error::Error + Send + Sync>> {
        let tensor_datos = preprocessing::preparar_para_reconocimiento(recorte);
        let forma: Vec<i64> = tensor_datos.shape().iter().map(|&d| d as i64).collect();
        let datos_flat: Vec<f32> = tensor_datos.as_standard_layout().iter().cloned().collect();

        let input_tensor = Tensor::from_array((forma, datos_flat))?;

        let mut sesion = self
            .sesion
            .lock()
            .map_err(|e| format!("Mutex poisoned: {}", e))?;
        let salidas = sesion.run(ort::inputs![input_tensor])?;

        let (forma_salida, datos) = salidas[0].try_extract_tensor::<f32>()?;
        let dims = &*forma_salida;

        let tamano_vocabulario = if dims.len() == 3 {
            dims[2] as usize
        } else {
            self.diccionario.len() + 1
        };

        let (texto, confianza) = ctc_decoder::decodificar_ctc_con_confianza(
            datos,
            tamano_vocabulario,
            &self.diccionario,
        );

        Ok(ResultadoReconocimiento { texto, confianza })
    }

    /// Reconoce múltiples recortes en una sola inferencia batch.
    ///
    /// El batching reduce overhead de despacho y aprovecha mejor GPU/CPU vectorial
    /// cuando un bloque genera varias líneas. El padding al ancho máximo es el
    /// costo asumido para obtener esa amortización.
    pub fn reconocer_batch(
        &self,
        recortes: &[DynamicImage],
    ) -> Result<Vec<ResultadoReconocimiento>, Box<dyn std::error::Error + Send + Sync>> {
        if recortes.is_empty() {
            return Ok(Vec::new());
        }
        if recortes.len() == 1 {
            return Ok(vec![self.reconocer(&recortes[0])?]);
        }

        const ALTURA: usize = 48;

        let tensores: Vec<ndarray::Array4<f32>> = recortes
            .iter()
            .map(|r| preprocessing::preparar_para_reconocimiento(r))
            .collect();

        let n = tensores.len();
        let ancho_max = tensores.iter().map(|t| t.shape()[3]).max().unwrap_or(1);

        let mut batch = ndarray::Array4::<f32>::zeros((n, 3, ALTURA, ancho_max));
        for (i, t) in tensores.iter().enumerate() {
            let w = t.shape()[3];
            batch
                .slice_mut(ndarray::s![i, .., .., ..w])
                .assign(&t.slice(ndarray::s![0, .., .., ..]));
        }

        let forma: Vec<i64> = vec![n as i64, 3, ALTURA as i64, ancho_max as i64];
        let datos_flat: Vec<f32> = batch.as_standard_layout().iter().cloned().collect();
        let input_tensor = Tensor::from_array((forma, datos_flat))?;

        let mut sesion = self
            .sesion
            .lock()
            .map_err(|e| format!("Mutex poisoned: {}", e))?;
        let salidas = sesion.run(ort::inputs![input_tensor])?;

        let (forma_salida, datos) = salidas[0].try_extract_tensor::<f32>()?;
        let dims = &*forma_salida;

        let vocab = if dims.len() == 3 {
            dims[2] as usize
        } else {
            self.diccionario.len() + 1
        };
        let t = datos.len() / (n * vocab);

        (0..n)
            .map(|i| {
                let offset = i * t * vocab;
                let slice = &datos[offset..offset + t * vocab];
                let (texto, confianza) =
                    ctc_decoder::decodificar_ctc_con_confianza(slice, vocab, &self.diccionario);
                Ok(ResultadoReconocimiento { texto, confianza })
            })
            .collect()
    }
}
