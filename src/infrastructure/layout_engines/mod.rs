//! Motores de analisis de layout.
//!
//! Este modulo contiene las implementaciones del puerto `LayoutEnginePort`
//! para detectar y segmentar regiones de contenido en paginas de documentos.

// OnnxLayoutEngine eliminado: era codigo muerto. El layout ONNX real es
// DocLayoutYoloEngine en infrastructure/ocr_engines/onnx/layout.rs.

use crate::domain::errors::LayoutError;
use crate::domain::{Block, BlockType, Page, Rectangle};
use crate::interfaces::ports::LayoutEnginePort;

/// Motor de layout basado en algoritmo XY-Cut.
///
/// Segmenta la pagina mediante proyecciones de pixeles oscuros en filas y columnas.
/// Detecta gaps (regiones vacias) para separar bloques de contenido de forma recursiva.
///
/// Es un algoritmo determinista, rapido y efectivo para documentos con
/// estructura columnar regular (texto corrido, dos columnas, titulos).
pub struct XyCutLayoutEngine {
    /// Ancho minimo en pixeles de un gap para ser considerado separador.
    min_gap_threshold: u32,
    /// Dimension minima de un bloque en pixeles para ser incluido.
    min_block_size: u32,
}

impl XyCutLayoutEngine {
    /// Crea un nuevo motor XY-Cut con parametros por defecto.
    pub fn new() -> Self {
        Self {
            min_gap_threshold: 20,
            min_block_size: 50,
        }
    }

    /// Crea un motor XY-Cut con parametros personalizados.
    pub fn with_params(min_gap_threshold: u32, min_block_size: u32) -> Self {
        Self { min_gap_threshold, min_block_size }
    }

    /// Calcula la proyeccion horizontal: numero de pixeles oscuros por fila.
    fn proyeccion_horizontal(&self, img: &image::GrayImage) -> Vec<u32> {
        let (ancho, alto) = img.dimensions();
        (0..alto)
            .map(|y| {
                (0..ancho)
                    .filter(|&x| img.get_pixel(x, y)[0] < 128)
                    .count() as u32
            })
            .collect()
    }

    /// Encuentra gaps (espacios vacios) en una proyeccion.
    ///
    /// Un gap es una secuencia consecutiva de valores por debajo del umbral
    /// con longitud >= `min_gap_threshold`.
    fn find_gaps(&self, proyeccion: &[u32], umbral: u32) -> Vec<(usize, usize)> {
        let mut gaps = Vec::new();
        let mut en_gap = false;
        let mut gap_inicio = 0;

        for (i, &valor) in proyeccion.iter().enumerate() {
            if valor <= umbral && !en_gap {
                en_gap = true;
                gap_inicio = i;
            } else if valor > umbral && en_gap {
                en_gap = false;
                if i - gap_inicio >= self.min_gap_threshold as usize {
                    gaps.push((gap_inicio, i));
                }
            }
        }

        if en_gap && proyeccion.len() - gap_inicio >= self.min_gap_threshold as usize {
            gaps.push((gap_inicio, proyeccion.len()));
        }

        gaps
    }

    /// Convierte una lista de gaps en regiones de contenido.
    ///
    /// Las regiones son los intervalos entre gaps.
    fn gaps_a_regiones(
        &self,
        gaps: &[(usize, usize)],
        inicio: usize,
        fin: usize,
    ) -> Vec<(usize, usize)> {
        let mut regiones = Vec::new();
        let mut pos = inicio;

        for &(gap_inicio, gap_fin) in gaps {
            if gap_inicio > pos {
                regiones.push((pos, gap_inicio));
            }
            pos = gap_fin;
        }

        if pos < fin {
            regiones.push((pos, fin));
        }

        regiones
    }
}

impl Default for XyCutLayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEnginePort for XyCutLayoutEngine {
    /// Analiza el layout de la pagina usando el algoritmo XY-Cut.
    ///
    /// Proyeccion horizontal → gaps de filas → proyeccion vertical por franja
    /// → gaps de columnas → bloques con tipo y orden de lectura asignados.
    fn analyze(&self, page: &Page) -> Result<Vec<Block>, LayoutError> {
        let image_data = match &page.image_data {
            Some(data) => data,
            None => {
                log::warn!("Pagina {} sin imagen, retornando bloques existentes", page.number);
                return Ok(page.blocks.clone());
            }
        };

        if page.dimensions.width < self.min_block_size
            || page.dimensions.height < self.min_block_size
        {
            log::warn!("Pagina {} demasiado pequena para analisis XY-Cut", page.number);
            return Ok(vec![]);
        }

        let img = image::load_from_memory(image_data)
            .map_err(|e| LayoutError::InvalidImage(e.to_string()))?
            .to_luma8();

        let (ancho, alto) = img.dimensions();

        // Umbral adaptativo: fila/columna con menos del 0.5% de pixeles oscuros = vacia
        let umbral_h = ((ancho as f32 * 0.005) as u32).max(1);
        let umbral_v = ((alto as f32 * 0.005) as u32).max(1);

        // Proyeccion horizontal y gaps entre bloques de texto
        let h_proj = self.proyeccion_horizontal(&img);
        let h_gaps = self.find_gaps(&h_proj, umbral_h);
        let regiones_y = self.gaps_a_regiones(&h_gaps, 0, alto as usize);

        let mut bloques: Vec<Block> = Vec::new();
        let mut orden_lectura = 0u32;

        for (y_ini, y_fin) in &regiones_y {
            if (y_fin - y_ini) < self.min_block_size as usize {
                continue;
            }

            // Proyeccion vertical sobre esta franja horizontal
            let v_proj_region: Vec<u32> = (0..ancho)
                .map(|x| {
                    (*y_ini as u32..*y_fin as u32)
                        .filter(|&y| img.get_pixel(x, y)[0] < 128)
                        .count() as u32
                })
                .collect();

            let v_gaps = self.find_gaps(&v_proj_region, umbral_v);
            let regiones_x = self.gaps_a_regiones(&v_gaps, 0, ancho as usize);

            for (x_ini, x_fin) in &regiones_x {
                if (x_fin - x_ini) < self.min_block_size as usize {
                    continue;
                }

                let bloque_ancho = (x_fin - x_ini) as u32;
                let bloque_alto = (y_fin - y_ini) as u32;

                let tipo = clasificar_bloque(
                    *y_ini as u32,
                    bloque_ancho,
                    bloque_alto,
                    ancho,
                    alto,
                );

                bloques.push(Block {
                    block_type: tipo,
                    bounding_box: Rectangle {
                        x: *x_ini as u32,
                        y: *y_ini as u32,
                        width: bloque_ancho,
                        height: bloque_alto,
                    },
                    content: String::new(),
                    confidence: 0.80,
                    embedded_image: None,
                    table_structure: None,
                    reading_order: orden_lectura,
                });

                orden_lectura += 1;
            }
        }

        log::info!(
            "XY-Cut detectó {} bloques en pagina {} ({}x{})",
            bloques.len(),
            page.number,
            ancho,
            alto
        );

        Ok(bloques)
    }

    fn name(&self) -> &str {
        "XyCutLayoutEngine"
    }
}

/// Clasifica un bloque segun su posicion y dimension en la pagina.
///
/// Heuristicas:
/// - `Title`: en el tercio superior Y ocupa mas de la mitad del ancho.
/// - `Separator`: alto < 10px (linea horizontal).
/// - `Text`: resto.
fn clasificar_bloque(
    y: u32,
    bloque_ancho: u32,
    bloque_alto: u32,
    pagina_ancho: u32,
    pagina_alto: u32,
) -> BlockType {
    if bloque_alto < 10 {
        return BlockType::Separator;
    }

    let en_tercio_superior = y < pagina_alto / 3;
    let cubre_mitad_ancho = bloque_ancho > pagina_ancho / 2;

    if en_tercio_superior && cubre_mitad_ancho {
        BlockType::Title
    } else {
        BlockType::Text
    }
}
