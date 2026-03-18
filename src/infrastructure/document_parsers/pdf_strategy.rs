use crate::domain::errors::DocumentError;
use crate::domain::Page;
use crate::interfaces::ports::PdfRendererPort;
use image::GenericImageView;
use std::path::Path;

use super::image_parser::{extension_normalizada, pagina_desde_imagen, DocumentParsingStrategy};

/// Estrategia para documentos PDF respaldados por un `PdfRendererPort`.
pub(super) struct PdfDocumentParsingStrategy {
    renderer: Option<Box<dyn PdfRendererPort>>,
}

impl PdfDocumentParsingStrategy {
    /// Construye la estrategia PDF con un renderer opcional.
    pub(super) fn new(renderer: Option<Box<dyn PdfRendererPort>>) -> Self {
        Self { renderer }
    }
}

impl DocumentParsingStrategy for PdfDocumentParsingStrategy {
    fn supports(&self, path: &Path) -> bool {
        extension_normalizada(path).as_deref() == Some("pdf")
    }

    fn source_format(&self) -> &'static str {
        "pdf"
    }

    fn parse_pages(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            DocumentError::PdfError(
                "libpdfium no disponible. Ejecute 'cargo build' para descargarla automaticamente."
                    .to_string(),
            )
        })?;

        let total_paginas = renderer.get_page_count(path)?;
        let mut paginas = Vec::with_capacity(total_paginas as usize);

        for numero_pagina in 1..=total_paginas {
            let imagen = renderer.render_page(path, numero_pagina)?;
            let _ = imagen.dimensions();
            paginas.push(pagina_desde_imagen(numero_pagina, imagen)?);
        }

        Ok(paginas)
    }
}
