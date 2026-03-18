use crate::domain::{Rectangle, TableCell, TableStructure};
use crate::infrastructure::ocr_engines::onnx::preprocessing;
use image::{DynamicImage, GenericImageView};
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

/// Analizador de estructura tabular basado en Table Transformer.
///
/// El componente asume que la región ya fue aislada por layout; su responsabilidad
/// es reconstruir filas y columnas internas. Esa separación reduce falsos
/// positivos y evita ejecutar un detector caro sobre la página completa.
pub struct TableAnalyzer {
    sesion: Mutex<Session>,
    umbral_confianza: f32,
}

#[derive(Debug, Clone)]
struct ComponenteTabla {
    clase: usize,
    caja: Rectangle,
    #[allow(dead_code)]
    confianza: f32,
}

impl TableAnalyzer {
    /// Carga el modelo Table Transformer desde disco.
    pub fn new(ruta_modelo: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let sesion = Session::builder()
            .and_then(|b| b.with_intra_threads(2))
            .and_then(|b| b.with_execution_providers(super::gpu_config::providers(0)))
            .and_then(|b| b.commit_from_file(ruta_modelo))
            .map_err(|e| format!("Error cargando Table Transformer: {}", e))?;

        log::info!("Table Transformer cargado desde: {:?}", ruta_modelo);

        Ok(Self {
            sesion: Mutex::new(sesion),
            umbral_confianza: 0.5,
        })
    }

    /// Analiza la estructura de una tabla a partir de un recorte ya localizado.
    pub fn analizar(
        &self,
        recorte_tabla: &DynamicImage,
    ) -> Result<TableStructure, Box<dyn std::error::Error + Send + Sync>> {
        let (ancho_original, alto_original) = recorte_tabla.dimensions();

        let tensor_datos = preprocessing::preparar_para_tabla(recorte_tabla);
        let forma: Vec<i64> = tensor_datos.shape().iter().map(|&d| d as i64).collect();
        let datos_flat: Vec<f32> = tensor_datos.as_standard_layout().iter().cloned().collect();

        let input_tensor = Tensor::from_array((forma, datos_flat))?;

        let mut sesion = self
            .sesion
            .lock()
            .map_err(|e| format!("Mutex poisoned: {}", e))?;
        let salidas = sesion.run(ort::inputs![input_tensor])?;

        let componentes = self.parsear_salida_detr(&salidas, ancho_original, alto_original)?;

        let filas: Vec<&ComponenteTabla> = componentes.iter().filter(|c| c.clase == 2).collect();
        let columnas: Vec<&ComponenteTabla> = componentes.iter().filter(|c| c.clase == 1).collect();

        let estructura = self.construir_estructura(&filas, &columnas);

        log::info!(
            "Table Transformer: {} filas x {} columnas",
            estructura.num_rows,
            estructura.num_cols
        );

        Ok(estructura)
    }

    /// Parsea la salida DETR: logits [1,N,C+1] y pred_boxes [1,N,4].
    fn parsear_salida_detr(
        &self,
        salidas: &ort::session::SessionOutputs,
        ancho_original: u32,
        alto_original: u32,
    ) -> Result<Vec<ComponenteTabla>, Box<dyn std::error::Error + Send + Sync>> {
        let (forma_logits, datos_logits) = salidas[0].try_extract_tensor::<f32>()?;
        let (_, datos_cajas) = salidas[1].try_extract_tensor::<f32>()?;

        let dims = &*forma_logits;
        let num_queries = dims[1] as usize;
        let num_clases_total = dims[2] as usize;
        let num_clases = num_clases_total - 1;

        let mut componentes = Vec::new();

        for q in 0..num_queries {
            let mut mejor_clase = 0usize;
            let mut mejor_score = f32::NEG_INFINITY;

            for c in 0..num_clases {
                let score = datos_logits[q * num_clases_total + c];
                if score > mejor_score {
                    mejor_score = score;
                    mejor_clase = c;
                }
            }

            let max_logit = (0..num_clases)
                .map(|c| datos_logits[q * num_clases_total + c])
                .fold(f32::NEG_INFINITY, f32::max);
            let exp_sum: f32 = (0..num_clases)
                .map(|c| (datos_logits[q * num_clases_total + c] - max_logit).exp())
                .sum();
            let confianza = (mejor_score - max_logit).exp() / exp_sum;
            if confianza < self.umbral_confianza {
                continue;
            }

            let cx = datos_cajas[q * 4];
            let cy = datos_cajas[q * 4 + 1];
            let w = datos_cajas[q * 4 + 2];
            let h = datos_cajas[q * 4 + 3];

            let x = ((cx - w / 2.0) * ancho_original as f32).max(0.0) as u32;
            let y = ((cy - h / 2.0) * alto_original as f32).max(0.0) as u32;

            componentes.push(ComponenteTabla {
                clase: mejor_clase,
                caja: Rectangle {
                    x,
                    y,
                    width: (w * ancho_original as f32) as u32,
                    height: (h * alto_original as f32) as u32,
                },
                confianza,
            });
        }

        Ok(componentes)
    }

    fn construir_estructura(
        &self,
        filas: &[&ComponenteTabla],
        columnas: &[&ComponenteTabla],
    ) -> TableStructure {
        if filas.is_empty() || columnas.is_empty() {
            return TableStructure {
                rows: Vec::new(),
                num_rows: 0,
                num_cols: 0,
            };
        }

        let mut filas_ord: Vec<&&ComponenteTabla> = filas.iter().collect();
        filas_ord.sort_by_key(|f| f.caja.y);

        let mut cols_ord: Vec<&&ComponenteTabla> = columnas.iter().collect();
        cols_ord.sort_by_key(|c| c.caja.x);

        let mut tabla_filas: Vec<Vec<TableCell>> = Vec::new();

        for fila in &filas_ord {
            let mut celdas: Vec<TableCell> = Vec::new();
            for col in &cols_ord {
                celdas.push(TableCell {
                    content: String::new(),
                    row_span: 1,
                    col_span: 1,
                    bounding_box: Rectangle {
                        x: col.caja.x,
                        y: fila.caja.y,
                        width: col.caja.width,
                        height: fila.caja.height,
                    },
                });
            }
            tabla_filas.push(celdas);
        }

        TableStructure {
            rows: tabla_filas,
            num_rows: filas_ord.len() as u32,
            num_cols: cols_ord.len() as u32,
        }
    }
}
