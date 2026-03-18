use crate::domain::errors::DocumentError;
use crate::domain::{Dimensions, Document, Page};
use crate::infrastructure::document_parsers::pdf_renderer::PdfiumRenderer;
use crate::interfaces::ports::{DocumentParserPort, PdfRendererPort};
use image::GenericImageView;
use std::collections::HashMap;
use std::path::Path;

/// Extensiones de imagen soportadas.
const EXTENSIONES_IMAGEN: &[&str] = &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp"];
/// Extensiones PDF soportadas.
const EXTENSIONES_PDF: &[&str] = &["pdf"];

/// Parser híbrido para imágenes directas y PDF rasterizado.
///
/// La implementación normaliza formatos heterogéneos a `Document + Vec<Page>` en
/// memoria, de modo que el resto del pipeline pueda operar sin distinguir si el
/// input original era raster o paginado. El soporte PDF se resuelve mediante
/// `PdfiumRenderer`, pero el parser degrada con gracia a modo solo-imágenes si
/// `pdfium` no está disponible.
///
/// # Trade-offs
///
/// El parser materializa cada página como PNG en memoria para simplificar
/// interoperabilidad entre fases. Esa decisión aumenta uso temporal de RAM, pero
/// evita formatos intermedios ad hoc y reduce dependencias entre módulos.
pub struct ImageDocumentParser {
    pdf_renderer: Option<PdfiumRenderer>,
}

impl ImageDocumentParser {
    /// Construye el parser y detecta si el soporte PDF está operativo.
    ///
    /// # Notes
    ///
    /// La ausencia de `pdfium` no se trata como error fatal porque la aplicación
    /// todavía puede operar sobre imágenes simples.
    pub fn new() -> Self {
        let pdf_renderer = match PdfiumRenderer::new() {
            Ok(r) => {
                log::info!("PdfiumRenderer inicializado correctamente");
                Some(r)
            }
            Err(e) => {
                log::warn!(
                    "Soporte PDF no disponible: {}. Solo se procesaran imagenes.",
                    e
                );
                None
            }
        };

        Self { pdf_renderer }
    }

    /// Verifica si es imagen.
    fn es_imagen(ruta: &Path) -> bool {
        ruta.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| EXTENSIONES_IMAGEN.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    /// Verifica si es PDF.
    fn es_pdf(ruta: &Path) -> bool {
        ruta.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| EXTENSIONES_PDF.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn procesar_imagen(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let es_tiff = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_lowercase().as_str(), "tiff" | "tif"))
            .unwrap_or(false);

        if es_tiff {
            self.procesar_tiff_multipagina(path)
        } else {
            self.procesar_imagen_simple(path)
        }
    }

    /// Carga una imagen simple (PNG, JPG, BMP, WebP) como pagina unica.
    fn procesar_imagen_simple(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let imagen = image::open(path)
            .map_err(|e| DocumentError::ImageError(format!("Error cargando imagen: {}", e)))?;

        let (ancho, alto) = imagen.dimensions();
        let bytes_png = Self::encode_png(&imagen)?;

        Ok(vec![Page {
            number: 1,
            dimensions: Dimensions {
                width: ancho,
                height: alto,
            },
            blocks: Vec::new(),
            image_data: Some(bytes_png),
        }])
    }

    /// Decodifica un TIFF y genera una Page por cada frame (IFD).
    ///
    /// Usa `tiff::Decoder` para iterar todas las paginas del TIFF
    /// sin importar cuantas haya. Cada pagina se convierte a PNG
    /// para ser procesada por el pipeline OCR.
    fn procesar_tiff_multipagina(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        use std::fs::File;
        use tiff::decoder::Decoder as TiffDecoder;
        use tiff::ColorType;

        let archivo = File::open(path)
            .map_err(|e| DocumentError::ImageError(format!("Error abriendo TIFF: {}", e)))?;
        let mut decoder = TiffDecoder::new(std::io::BufReader::new(archivo))
            .map_err(|e| DocumentError::ImageError(format!("Error creando decoder TIFF: {}", e)))?;

        let mut paginas = Vec::new();
        let mut numero_pagina: u32 = 1;

        loop {
            let (ancho, alto) = decoder.dimensions().map_err(|e| {
                DocumentError::ImageError(format!(
                    "Error leyendo dimensiones TIFF p{}: {}",
                    numero_pagina, e
                ))
            })?;

            let imagen_dyn = match decoder.colortype().map_err(|e| {
                DocumentError::ImageError(format!("Error leyendo color type TIFF: {}", e))
            })? {
                ColorType::RGB(8) | ColorType::RGBA(8) => {
                    let datos = decoder.read_image().map_err(|e| {
                        DocumentError::ImageError(format!(
                            "Error leyendo pixeles TIFF p{}: {}",
                            numero_pagina, e
                        ))
                    })?;
                    let pixeles = match datos {
                        tiff::decoder::DecodingResult::U8(v) => v,
                        other => {
                            return Err(DocumentError::ImageError(format!(
                                "Formato de pixel TIFF inesperado en p{}: {:?}",
                                numero_pagina, other
                            )));
                        }
                    };
                    image::DynamicImage::ImageRgb8(
                        image::RgbImage::from_raw(ancho, alto, pixeles).ok_or_else(|| {
                            DocumentError::ImageError(format!(
                                "Buffer insuficiente para TIFF p{}",
                                numero_pagina
                            ))
                        })?,
                    )
                }
                _ => {
                    if numero_pagina == 1 {
                        image::open(path).map_err(|e| {
                            DocumentError::ImageError(format!("Fallback TIFF p1: {}", e))
                        })?
                    } else {
                        break;
                    }
                }
            };

            let bytes_png = Self::encode_png(&imagen_dyn)?;

            paginas.push(Page {
                number: numero_pagina,
                dimensions: Dimensions {
                    width: ancho,
                    height: alto,
                },
                blocks: Vec::new(),
                image_data: Some(bytes_png),
            });

            if decoder.next_image().is_err() {
                break;
            }

            numero_pagina += 1;
        }

        if paginas.is_empty() {
            return Err(DocumentError::ImageError(
                "TIFF sin paginas decodificables".to_string(),
            ));
        }

        log::info!(
            "TIFF multi-pagina: {} paginas extraidas de {:?}",
            paginas.len(),
            path
        );
        Ok(paginas)
    }

    /// Codifica una DynamicImage a bytes PNG.
    fn encode_png(imagen: &image::DynamicImage) -> Result<Vec<u8>, DocumentError> {
        let mut bytes_png = std::io::Cursor::new(Vec::new());
        imagen
            .write_to(&mut bytes_png, image::ImageFormat::Png)
            .map_err(|e| DocumentError::ImageError(format!("Error codificando PNG: {}", e)))?;
        Ok(bytes_png.into_inner())
    }

    fn procesar_pdf(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let renderer = self.pdf_renderer.as_ref().ok_or_else(|| {
            DocumentError::PdfError(
                "libpdfium no disponible. Ejecute 'cargo build' para descargarla automaticamente."
                    .to_string(),
            )
        })?;

        let num_paginas = renderer.get_page_count(path)?;
        let mut pages = Vec::new();

        for i in 1..=num_paginas {
            let imagen = renderer.render_page(path, i)?;
            let (ancho, alto) = imagen.dimensions();

            let mut bytes_png = std::io::Cursor::new(Vec::new());
            imagen
                .write_to(&mut bytes_png, image::ImageFormat::Png)
                .map_err(|e| {
                    DocumentError::ImageError(format!("Error codificando pagina PDF {}: {}", i, e))
                })?;

            pages.push(Page {
                number: i,
                dimensions: Dimensions {
                    width: ancho,
                    height: alto,
                },
                blocks: Vec::new(),
                image_data: Some(bytes_png.into_inner()),
            });
        }

        Ok(pages)
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

        let is_pdf = Self::es_pdf(path);
        let is_img = Self::es_imagen(path);

        if !is_pdf && !is_img {
            return Err(DocumentError::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("desconocido")
                    .to_string(),
            ));
        }

        log::info!("Parseando documento: {:?}", path);

        let start = std::time::Instant::now();
        let pages = if is_pdf {
            self.procesar_pdf(path)?
        } else {
            self.procesar_imagen(path)?
        };

        log::info!(
            "Documento parseado en {:.2}s ({} paginas)",
            start.elapsed().as_secs_f32(),
            pages.len()
        );

        let mut metadata = HashMap::new();
        metadata.insert(
            "source_format".into(),
            if is_pdf { "pdf".into() } else { "image".into() },
        );
        metadata.insert(
            "filename".to_string(),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
        );

        Ok(Document {
            id: uuid::Uuid::new_v4().to_string(),
            source_path: path.to_path_buf(),
            pages,
            metadata,
        })
    }
}
