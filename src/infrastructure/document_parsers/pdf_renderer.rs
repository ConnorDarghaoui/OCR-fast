use crate::domain::errors::DocumentError;
use crate::interfaces::ports::PdfRendererPort;
use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::Path;

/// Adaptador PDF respaldado por Google Pdfium.
///
/// El renderer concentra la dependencia nativa en un único punto del sistema.
/// Eso evita que el parser o el pipeline conozcan detalles de carga dinámica y
/// permite degradar el soporte PDF sin romper el resto de la aplicación.
///
/// # Trade-offs
///
/// Cada operación vuelve a resolver bindings para privilegiar robustez de runtime
/// y claridad diagnóstica sobre caching agresivo de handles globales.
pub struct PdfiumRenderer;

impl PdfiumRenderer {
    /// Verifica disponibilidad de Pdfium y construye el adaptador.
    ///
    /// # Errors
    ///
    /// Falla si la biblioteca dinámica no puede localizarse o enlazarse.
    pub fn new() -> Result<Self, DocumentError> {
        // Verificar disponibilidad inmediata al construir
        Self::obtener_bindings()?;
        Ok(Self)
    }

    /// Obtener bindings de Pdfium buscando la libreria en multiples ubicaciones.
    fn obtener_bindings() -> Result<Pdfium, DocumentError> {
        // 1. Buscar junto al ejecutable
        let junto_a_exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        let resultado = junto_a_exe
            .and_then(|dir| {
                let nombre_lib =
                    Pdfium::pdfium_platform_library_name_at_path(dir.to_str().unwrap_or("./"));
                Pdfium::bind_to_library(nombre_lib).ok()
            })
            .or_else(|| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./")).ok()
            })
            .or_else(|| Pdfium::bind_to_system_library().ok());

        match resultado {
            Some(bindings) => Ok(Pdfium::new(bindings)),
            None => Err(DocumentError::PdfError(
                "libpdfium no encontrada. Ejecute 'cargo build' para descargarla automaticamente, \
                 o coloque libpdfium.so junto al ejecutable."
                    .to_string(),
            )),
        }
    }
}

impl PdfRendererPort for PdfiumRenderer {
    /// Rasteriza una página PDF a imagen en memoria.
    fn render_page(&self, path: &Path, page_number: u32) -> Result<DynamicImage, DocumentError> {
        let pdfium = Self::obtener_bindings()?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| DocumentError::PdfError(format!("Error abriendo PDF: {}", e)))?;

        // page_number llega 1-based, Pdfium usa 0-based
        let page_index = (page_number as u16).saturating_sub(1);

        if page_index >= document.pages().len() {
            return Err(DocumentError::PdfError(format!(
                "Pagina {} fuera de rango (total: {})",
                page_number,
                document.pages().len()
            )));
        }

        let page = document
            .pages()
            .get(page_index)
            .map_err(|e| DocumentError::PdfError(format!("Error accediendo pagina: {}", e)))?;

        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(2000))
            .map_err(|e| DocumentError::PdfError(format!("Error renderizando: {}", e)))?;

        Ok(bitmap.as_image())
    }

    /// Obtiene el número de páginas sin renderizar el documento completo.
    fn get_page_count(&self, path: &Path) -> Result<u32, DocumentError> {
        let pdfium = Self::obtener_bindings()?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| DocumentError::PdfError(format!("Error abriendo PDF: {}", e)))?;

        Ok(document.pages().len() as u32)
    }
}
