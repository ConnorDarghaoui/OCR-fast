//! Servicios de preprocesamiento de imagenes.
//!
//! Orden de aplicacion optimo: denoise → binarize → deskew.

use crate::domain::errors::PreprocessError;
use crate::domain::Document;
use crate::interfaces::ports::PreprocessorPort;

/// Preprocesador de imagenes con pipeline configurable.
pub struct ImagePreprocessor {
    /// Aplicar binarizacion Otsu.
    apply_binarization: bool,
    /// Aplicar correccion de inclinacion.
    apply_deskew: bool,
    /// Aplicar eliminacion de ruido gaussiano.
    apply_denoise: bool,
    /// DPI objetivo (informativo; la normalizacion de DPI requiere metadatos del scanner).
    target_dpi: u32,
}

impl ImagePreprocessor {
    /// Crea un preprocesador con configuracion por defecto.
    pub fn new() -> Self {
        Self {
            apply_binarization: true,
            apply_deskew: true,
            apply_denoise: true,
            target_dpi: 300,
        }
    }

    /// Crea un preprocesador con configuracion personalizada.
    pub fn with_config(
        apply_binarization: bool,
        apply_deskew: bool,
        apply_denoise: bool,
        target_dpi: u32,
    ) -> Self {
        Self { apply_binarization, apply_deskew, apply_denoise, target_dpi }
    }

    /// Elimina ruido gaussiano con suavizado ligero (sigma=0.8).
    ///
    /// Se aplica sobre la imagen en escala de grises antes de binarizar.
    /// Un sigma de 0.8 elimina ruido de alta frecuencia sin borrar bordes de texto.
    fn denoise(&self, image_data: &mut Vec<u8>) -> Result<(), PreprocessError> {
        let img = image::load_from_memory(image_data)
            .map_err(|e| PreprocessError::DenoiseError(e.to_string()))?
            .to_luma8();

        let suavizada = image::imageops::blur(&img, 0.8);

        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(suavizada)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .map_err(|e| PreprocessError::DenoiseError(e.to_string()))?;

        *image_data = buffer.into_inner();
        log::debug!("Denoise aplicado (sigma=0.8)");
        Ok(())
    }

    /// Binariza la imagen usando el metodo de Otsu.
    ///
    /// Calcula automaticamente el umbral optimo que maximiza la varianza
    /// inter-clase entre fondo (blanco) y texto (negro).
    fn binarize(&self, image_data: &mut Vec<u8>) -> Result<(), PreprocessError> {
        let img = image::load_from_memory(image_data)
            .map_err(|e| PreprocessError::BinarizationError(e.to_string()))?
            .to_luma8();

        let (ancho, alto) = img.dimensions();

        // Histograma de 256 niveles
        let mut histograma = [0u32; 256];
        for pixel in img.pixels() {
            histograma[pixel[0] as usize] += 1;
        }

        // Suma total ponderada de intensidades
        let total_pixeles = (ancho * alto) as f64;
        let suma_total: f64 = (0..256usize)
            .map(|i| i as f64 * histograma[i] as f64)
            .sum();

        // Metodo de Otsu: maximizar varianza inter-clase
        let mut mejor_umbral = 128u8;
        let mut mejor_varianza = 0f64;
        let mut peso_fondo = 0f64;
        let mut suma_fondo = 0f64;

        for umbral in 0..256u32 {
            peso_fondo += histograma[umbral as usize] as f64;
            if peso_fondo == 0.0 {
                continue;
            }

            let peso_frente = total_pixeles - peso_fondo;
            if peso_frente == 0.0 {
                break;
            }

            suma_fondo += umbral as f64 * histograma[umbral as usize] as f64;
            let media_fondo = suma_fondo / peso_fondo;
            let media_frente = (suma_total - suma_fondo) / peso_frente;

            let varianza = peso_fondo * peso_frente * (media_fondo - media_frente).powi(2);
            if varianza > mejor_varianza {
                mejor_varianza = varianza;
                mejor_umbral = umbral as u8;
            }
        }

        // Aplicar umbral binario
        let mut binarizada = image::GrayImage::new(ancho, alto);
        for (x, y, pixel) in img.enumerate_pixels() {
            let valor = if pixel[0] >= mejor_umbral { 255u8 } else { 0u8 };
            binarizada.put_pixel(x, y, image::Luma([valor]));
        }

        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(binarizada)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .map_err(|e| PreprocessError::BinarizationError(e.to_string()))?;

        *image_data = buffer.into_inner();
        log::debug!("Binarizacion Otsu aplicada (umbral={})", mejor_umbral);
        Ok(())
    }

    /// Corrige la inclinacion de la imagen mediante busqueda en dos pasadas.
    ///
    /// Busqueda gruesa (-10°..+10° en pasos de 1°), luego refinamiento fino
    /// (±1° en pasos de 0.2°). Se omite si la correccion esta dentro de 0.2°.
    fn deskew(&self, image_data: &mut Vec<u8>) -> Result<(), PreprocessError> {
        let img_original = image::load_from_memory(image_data)
            .map_err(|e| PreprocessError::DeskewError(e.to_string()))?
            .to_luma8();

        let (ancho_orig, alto_orig) = img_original.dimensions();

        // Reducir para busqueda de angulo eficiente
        const ANCHO_BUSQUEDA: u32 = 500;
        let img_pequena = if ancho_orig > ANCHO_BUSQUEDA {
            let alto_escala = (alto_orig as f64 * ANCHO_BUSQUEDA as f64 / ancho_orig as f64) as u32;
            image::imageops::resize(
                &img_original,
                ANCHO_BUSQUEDA,
                alto_escala.max(1),
                image::imageops::FilterType::Nearest,
            )
        } else {
            img_original.clone()
        };

        // Busqueda gruesa: -10 a +10 grados en pasos de 1.0
        let mut mejor_angulo = 0.0f64;
        let mut mejor_varianza = varianza_proyeccion_h(&img_pequena);

        for grados_x10 in -100i32..=100i32 {
            let angulo = grados_x10 as f64 / 10.0;
            let rotada = rotar_imagen_gris(&img_pequena, angulo.to_radians());
            let varianza = varianza_proyeccion_h(&rotada);
            if varianza > mejor_varianza {
                mejor_varianza = varianza;
                mejor_angulo = angulo;
            }
        }

        // Busqueda fina: ±1.0 grado alrededor del mejor angulo en pasos de 0.2
        let inicio_fino = (mejor_angulo - 1.0).max(-15.0);
        let fin_fino = (mejor_angulo + 1.0).min(15.0);
        let mut angulo_fino = inicio_fino;
        while angulo_fino <= fin_fino {
            let rotada = rotar_imagen_gris(&img_pequena, angulo_fino.to_radians());
            let varianza = varianza_proyeccion_h(&rotada);
            if varianza > mejor_varianza {
                mejor_varianza = varianza;
                mejor_angulo = angulo_fino;
            }
            angulo_fino += 0.2;
        }

        // Tolerancia: no corregir si el angulo es menor a 0.2 grados
        if mejor_angulo.abs() < 0.2 {
            log::debug!("Deskew: angulo {:.2}° dentro de tolerancia, sin correccion", mejor_angulo);
            return Ok(());
        }

        log::debug!("Deskew: corrigiendo {:.2}°", mejor_angulo);

        // Aplicar rotacion correctiva sobre la imagen a resolucion completa
        let corregida = rotar_imagen_gris(&img_original, mejor_angulo.to_radians());

        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(corregida)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .map_err(|e| PreprocessError::DeskewError(e.to_string()))?;

        *image_data = buffer.into_inner();
        Ok(())
    }
}

/// Rota una imagen en escala de grises por el angulo dado (en radianes).
///
/// Usa mapeo inverso con interpolacion nearest-neighbor.
/// El fondo (areas fuera del original) se rellena con blanco (255).
fn rotar_imagen_gris(img: &image::GrayImage, angulo_rad: f64) -> image::GrayImage {
    let (ancho, alto) = img.dimensions();
    let mut rotada = image::GrayImage::new(ancho, alto);

    let cos_a = angulo_rad.cos();
    let sin_a = angulo_rad.sin();
    let cx = ancho as f64 / 2.0;
    let cy = alto as f64 / 2.0;

    for y in 0..alto {
        for x in 0..ancho {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;

            // Mapeo inverso: coordenadas en la imagen original
            let ox = (dx * cos_a + dy * sin_a + cx).round() as i64;
            let oy = (-dx * sin_a + dy * cos_a + cy).round() as i64;

            let valor = if ox >= 0 && ox < ancho as i64 && oy >= 0 && oy < alto as i64 {
                img.get_pixel(ox as u32, oy as u32)[0]
            } else {
                255u8 // Fondo blanco
            };

            rotada.put_pixel(x, y, image::Luma([valor]));
        }
    }

    rotada
}

/// Calcula la varianza de la proyeccion horizontal (suma de pixeles oscuros por fila).
///
/// Una varianza alta indica filas de texto bien alineadas (picos marcados en la proyeccion).
/// Se usa como metrica de calidad para la busqueda de angulo en el deskew.
fn varianza_proyeccion_h(img: &image::GrayImage) -> f64 {
    let (ancho, alto) = img.dimensions();
    if alto == 0 {
        return 0.0;
    }

    let proyeccion: Vec<f64> = (0..alto)
        .map(|y| {
            (0..ancho)
                .filter(|&x| img.get_pixel(x, y)[0] < 128)
                .count() as f64
        })
        .collect();

    let media = proyeccion.iter().sum::<f64>() / proyeccion.len() as f64;
    proyeccion.iter().map(|&v| (v - media).powi(2)).sum::<f64>() / proyeccion.len() as f64
}

impl Default for ImagePreprocessor {
    fn default() -> Self {
        Self::new()
    }
}

impl PreprocessorPort for ImagePreprocessor {
    /// Aplica el pipeline de preprocesamiento en orden optimo:
    /// denoise → binarize → deskew.
    fn preprocess(&self, document: &mut Document) -> Result<(), PreprocessError> {
        log::info!(
            "Preprocesando documento {} ({} paginas, target_dpi={})",
            document.id,
            document.pages.len(),
            self.target_dpi
        );

        for page in &mut document.pages {
            if let Some(ref mut image_data) = page.image_data {
                log::debug!("Preprocesando pagina {}", page.number);

                // Orden: denoise primero (imagen original), luego binarizar, luego deskew
                if self.apply_denoise {
                    self.denoise(image_data)?;
                }

                if self.apply_binarization {
                    self.binarize(image_data)?;
                }

                if self.apply_deskew {
                    self.deskew(image_data)?;
                }
            } else {
                log::warn!("Pagina {} sin datos de imagen, omitiendo", page.number);
            }
        }

        log::info!("Preprocesamiento completado");
        Ok(())
    }
}
