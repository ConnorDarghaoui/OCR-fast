use crate::domain::errors::DocumentError;
use crate::domain::Page;
use image::{DynamicImage, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tiff::decoder::{Decoder as TiffDecoder, DecodingResult};
use tiff::ColorType;

use super::image_parser::{extension_normalizada, pagina_desde_imagen, DocumentParsingStrategy};

const EXTENSIONES_TIFF: &[&str] = &["tif", "tiff"];

/// Estrategia para TIFF de una o múltiples páginas.
pub(super) struct TiffImageParsingStrategy;

impl TiffImageParsingStrategy {
    /// Construye la estrategia TIFF sin estado mutable.
    pub(super) fn new() -> Self {
        Self
    }

    /// Decodifica la imagen TIFF actual preservando su representación soportada.
    fn decodificar_imagen_actual(
        decoder: &mut TiffDecoder<BufReader<File>>,
        ancho: u32,
        alto: u32,
        numero_pagina: u32,
    ) -> Result<DynamicImage, DocumentError> {
        let tipo_color = decoder.colortype().map_err(|e| {
            DocumentError::ImageError(format!("Error leyendo color type TIFF: {}", e))
        })?;

        match tipo_color {
            ColorType::Gray(8) => {
                let pixeles = Self::leer_pixeles_u8(decoder, numero_pagina)?;
                let imagen = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(ancho, alto, pixeles)
                    .ok_or_else(|| {
                        DocumentError::ImageError(format!(
                            "Buffer insuficiente para TIFF p{}",
                            numero_pagina
                        ))
                    })?;
                Ok(DynamicImage::ImageLuma8(imagen))
            }
            ColorType::GrayA(8) => {
                let pixeles = Self::leer_pixeles_u8(decoder, numero_pagina)?;
                let imagen = ImageBuffer::<LumaA<u8>, Vec<u8>>::from_raw(ancho, alto, pixeles)
                    .ok_or_else(|| {
                        DocumentError::ImageError(format!(
                            "Buffer insuficiente para TIFF p{}",
                            numero_pagina
                        ))
                    })?;
                Ok(DynamicImage::ImageLumaA8(imagen))
            }
            ColorType::RGB(8) => {
                let pixeles = Self::leer_pixeles_u8(decoder, numero_pagina)?;
                let imagen = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(ancho, alto, pixeles)
                    .ok_or_else(|| {
                        DocumentError::ImageError(format!(
                            "Buffer insuficiente para TIFF p{}",
                            numero_pagina
                        ))
                    })?;
                Ok(DynamicImage::ImageRgb8(imagen))
            }
            ColorType::RGBA(8) => {
                let pixeles = Self::leer_pixeles_u8(decoder, numero_pagina)?;
                let imagen = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(ancho, alto, pixeles)
                    .ok_or_else(|| {
                        DocumentError::ImageError(format!(
                            "Buffer insuficiente para TIFF p{}",
                            numero_pagina
                        ))
                    })?;
                Ok(DynamicImage::ImageRgba8(imagen))
            }
            other => Err(DocumentError::ImageError(format!(
                "Color TIFF no soportado en p{}: {:?}",
                numero_pagina, other
            ))),
        }
    }

    /// Lee pixeles de 8 bits para la página TIFF actual.
    fn leer_pixeles_u8(
        decoder: &mut TiffDecoder<BufReader<File>>,
        numero_pagina: u32,
    ) -> Result<Vec<u8>, DocumentError> {
        match decoder.read_image().map_err(|e| {
            DocumentError::ImageError(format!(
                "Error leyendo pixeles TIFF p{}: {}",
                numero_pagina, e
            ))
        })? {
            DecodingResult::U8(pixeles) => Ok(pixeles),
            other => Err(DocumentError::ImageError(format!(
                "Formato de pixel TIFF inesperado en p{}: {:?}",
                numero_pagina, other
            ))),
        }
    }
}

impl DocumentParsingStrategy for TiffImageParsingStrategy {
    fn supports(&self, path: &Path) -> bool {
        extension_normalizada(path)
            .as_deref()
            .map(|extension| EXTENSIONES_TIFF.contains(&extension))
            .unwrap_or(false)
    }

    fn source_format(&self) -> &'static str {
        "image"
    }

    fn parse_pages(&self, path: &Path) -> Result<Vec<Page>, DocumentError> {
        let archivo = File::open(path)
            .map_err(|e| DocumentError::ImageError(format!("Error abriendo TIFF: {}", e)))?;
        let mut decoder = TiffDecoder::new(BufReader::new(archivo))
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

            let imagen = Self::decodificar_imagen_actual(&mut decoder, ancho, alto, numero_pagina)?;
            paginas.push(pagina_desde_imagen(numero_pagina, imagen)?);

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
}
