use crate::domain::errors::DocumentError;
use crate::domain::{Dimensions, Document, Page};
use crate::infrastructure::document_parsers::pdf_renderer::PdfiumRenderer;
use crate::interfaces::ports::{DocumentParserPort, PdfRendererPort};
use image::GenericImageView;
use std::collections::HashMap;
use std::path::Path;

use super::pdf_strategy::PdfDocumentParsingStrategy;
use super::raster_strategy::RasterImageParsingStrategy;
use super::tiff_strategy::TiffImageParsingStrategy;

/// Estrategia interna para normalizar distintos formatos a páginas raster.
pub(super) trait DocumentParsingStrategy: Send + Sync {
    /// Indica si la estrategia puede procesar la ruta indicada.
    fn supports(&self, path: &Path) -> bool;
    /// Describe el formato de origen usado para metadatos operativos.
    fn source_format(&self) -> &'static str;
    /// Extrae páginas rasterizadas listas para el pipeline OCR.
    fn parse_pages(&self, path: &Path) -> Result<Vec<Page>, DocumentError>;
}

/// Parser coordinador para imágenes raster, TIFF y PDF.
///
/// La estructura ya no concentra la lógica de cada formato. Actúa como un
/// selector de estrategia para mantener acotadas las ramas de decisión y dejar
/// que cada backend resuelva su propia complejidad de decodificación.
pub struct ImageDocumentParser {
    estrategias: Vec<Box<dyn DocumentParsingStrategy>>,
}

impl ImageDocumentParser {
    /// Construye el parser y registra las estrategias soportadas.
    ///
    /// # Notes
    ///
    /// El soporte PDF permanece degradable: si `pdfium` no está disponible, la
    /// estrategia PDF sigue registrándose pero devolverá un error explícito al
    /// parsear PDFs en lugar de degradar silenciosamente a otro formato.
    pub fn new() -> Self {
        let pdf_renderer: Option<Box<dyn PdfRendererPort>> = match PdfiumRenderer::new() {
            Ok(renderer) => {
                log::info!("PdfiumRenderer inicializado correctamente");
                Some(Box::new(renderer))
            }
            Err(error) => {
                log::warn!(
                    "Soporte PDF no disponible: {}. Solo se procesaran imagenes raster/TIFF.",
                    error
                );
                None
            }
        };

        let estrategias: Vec<Box<dyn DocumentParsingStrategy>> = vec![
            Box::new(PdfDocumentParsingStrategy::new(pdf_renderer)),
            Box::new(TiffImageParsingStrategy::new()),
            Box::new(RasterImageParsingStrategy::new()),
        ];

        Self { estrategias }
    }
}

impl Default for ImageDocumentParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentParserPort for ImageDocumentParser {
    /// Convierte un archivo físico a la representación de dominio inicial.
    ///
    /// # Errors
    ///
    /// Retorna `DocumentError` cuando el recurso no existe, no tiene un formato
    /// soportado o no puede decodificarse a páginas raster válidas.
    fn parse(&self, path: &Path) -> Result<Document, DocumentError> {
        if !path.exists() {
            return Err(DocumentError::NotFound(path.to_path_buf()));
        }

        let estrategia = self
            .estrategias
            .iter()
            .find(|estrategia| estrategia.supports(path));
        let estrategia = estrategia.ok_or_else(|| {
            DocumentError::UnsupportedFormat(
                extension_normalizada(path).unwrap_or_else(|| "desconocido".to_string()),
            )
        })?;

        log::info!("Parseando documento: {:?}", path);

        let inicio = std::time::Instant::now();
        let paginas = estrategia.parse_pages(path)?;

        log::info!(
            "Documento parseado en {:.2}s ({} paginas)",
            inicio.elapsed().as_secs_f32(),
            paginas.len()
        );

        let mut metadata = HashMap::new();
        metadata.insert(
            "source_format".to_string(),
            estrategia.source_format().to_string(),
        );
        metadata.insert(
            "filename".to_string(),
            path.file_name()
                .and_then(|nombre| nombre.to_str())
                .unwrap_or("unknown")
                .to_string(),
        );

        Ok(Document {
            id: uuid::Uuid::new_v4().to_string(),
            source_path: path.to_path_buf(),
            pages: paginas,
            metadata,
        })
    }
}

/// Normaliza la extensión de archivo a minúsculas para resolución de estrategia.
pub(super) fn extension_normalizada(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
}

/// Convierte una imagen arbitraria a la `Page` canónica usada por el dominio.
pub(super) fn pagina_desde_imagen(
    numero_pagina: u32,
    imagen: image::DynamicImage,
) -> Result<Page, DocumentError> {
    let (ancho, alto) = imagen.dimensions();
    let bytes_png = encode_png(&imagen)?;

    Ok(Page {
        number: numero_pagina,
        dimensions: Dimensions {
            width: ancho,
            height: alto,
        },
        blocks: Vec::new(),
        image_data: Some(bytes_png),
    })
}

/// Codifica una `DynamicImage` a PNG para consumo uniforme del pipeline.
pub(super) fn encode_png(imagen: &image::DynamicImage) -> Result<Vec<u8>, DocumentError> {
    let mut bytes_png = std::io::Cursor::new(Vec::new());
    imagen
        .write_to(&mut bytes_png, image::ImageFormat::Png)
        .map_err(|e| DocumentError::ImageError(format!("Error codificando PNG: {}", e)))?;
    Ok(bytes_png.into_inner())
}
