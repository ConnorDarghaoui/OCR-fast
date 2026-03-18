use crate::domain::errors::DocumentError;
use crate::domain::Page;
use std::path::Path;

use super::image_parser::{extension_normalizada, pagina_desde_imagen, DocumentParsingStrategy};

const EXTENSIONES_RASTER: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp"];

/// Estrategia para imágenes raster de página única.
pub(super) struct RasterImageParsingStrategy;

impl RasterImageParsingStrategy {
    /// Construye la estrategia sin estado para imágenes raster.
    pub(super) fn new() -> Self {
        Self
    }
}

impl DocumentParsingStrategy for RasterImageParsingStrategy {
    fn supports(&self, path: &Path) -> bool {
        extension_normalizada(path)
            .as_deref()
            .map(|extension| EXTENSIONES_RASTER.contains(&extension))
            .unwrap_or(false)
    }

    fn source_format(&self) -> &'static str {
        "image"
    }

    fn parse_pages(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let imagen = image::open(path)
            .map_err(|e| DocumentError::ImageError(format!("Error cargando imagen: {}", e)))?;

        Ok(vec![pagina_desde_imagen(1, imagen)?])
    }
}
