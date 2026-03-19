use super::common::{
    asegurar_directorio_padre, construir_blueprint, obtener_pagina, px_a_pt,
    recortar_imagen_desde_referencia,
};
use crate::domain::errors::ExportError;
use crate::domain::{
    AlignmentHint, ElementBlueprint, ElementRole, EmphasisHint, Job, PageBlueprint,
    ProcessingMode, Rectangle, TableCellAlignment, TableStructure,
};
use crate::interfaces::ports::ExporterPort;
use encoding_rs::WINDOWS_1252;
use lopdf::dictionary;
use std::io::Cursor;
use std::path::Path;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Fracción de alto del bounding box usada como tamaño de fuente PDF.
const FACTOR_TAMANO_FUENTE: f64 = 0.8;
/// Tamaño mínimo de fuente PDF.
const TAMANO_FUENTE_MINIMO_PT: f64 = 6.0;
/// Tamaño máximo de fuente PDF.
const TAMANO_FUENTE_MAXIMO_PT: f64 = 72.0;
/// Calidad JPEG para recortes embebidos en PDF reconstruido.
const PDF_IMAGE_JPEG_QUALITY: u8 = 82;
/// Umbral conservador para degradar bloques OCR débiles a imagen en PDF.
const PDF_FALLBACK_OCR_CONFIDENCE_THRESHOLD: f32 = 0.74;
/// La fuente core Helvetica usa una caja de 1000 unidades por EM.
const HELVETICA_UNIDADES_POR_EM: f64 = 1000.0;
/// Fallback conservador mientras el camino Unicode siga sin fuente embebida.
const HELVETICA_GLYPH_WIDTH_FALLBACK: u16 = 556;
/// Métricas estándar de Helvetica para ASCII printable (32..=126).
const HELVETICA_GLYPH_WIDTHS_ASCII: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Exportador a PDF reconstruido a partir del blueprint visual.
pub struct PdfReconstructedExporter;

impl PdfReconstructedExporter {
    /// Construye un exportador PDF reconstruido sin estado mutable.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PdfReconstructedExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for PdfReconstructedExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        use lopdf::content::Content;
        use lopdf::{Document, Object};

        asegurar_directorio_padre(output_path)?;

        let blueprint = construir_blueprint(&job.document)?;
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut page_ids: Vec<Object> = Vec::new();

        for pagina in &blueprint.pages {
            let ancho_pt = px_a_pt(pagina.dimensions.width);
            let alto_pt = px_a_pt(pagina.dimensions.height);
            let mut operaciones = Vec::new();
            let mut recursos_xobject = lopdf::Dictionary::new();

            if blueprint.processing_mode == ProcessingMode::VisualPreservation
                && agregar_fondo_pagina_visual_pdf(
                    job,
                    pagina.number,
                    pagina.dimensions.width,
                    pagina.dimensions.height,
                    &mut doc,
                    &mut recursos_xobject,
                    &mut operaciones,
                )?
            {
                for elemento in &pagina.elements {
                    agregar_texto_pdf_invisible(pagina, elemento, &mut operaciones);
                }
            } else {
                for (indice_elemento, elemento) in pagina.elements.iter().enumerate() {
                    agregar_elemento_pdf(
                        job,
                        pagina,
                        elemento,
                        indice_elemento,
                        &mut doc,
                        &mut recursos_xobject,
                        &mut operaciones,
                    )?;
                }
            }
            let content = Content {
                operations: operaciones,
            };
            let content_id = doc.add_object(lopdf::Stream::new(
                dictionary! {},
                content.encode().map_err(|e| {
                    ExportError::SerializationError(format!(
                        "Error codificando content stream: {e}"
                    ))
                })?,
            ));

            let mut resources_dict = lopdf::Dictionary::new();
            resources_dict.set(
                "Font",
                lopdf::dictionary! {
                    "F1" => font_id,
                },
            );
            if !recursos_xobject.is_empty() {
                resources_dict.set("XObject", lopdf::Object::Dictionary(recursos_xobject));
            }

            let resources_id = doc.add_object(lopdf::Object::Dictionary(resources_dict));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![
                    0.into(),
                    0.into(),
                    lopdf::Object::Real(ancho_pt as f32),
                    lopdf::Object::Real(alto_pt as f32),
                ],
            });
            page_ids.push(page_id.into());
        }

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids,
            "Count" => blueprint.pages.len() as u32,
        };
        doc.objects
            .insert(pages_id, lopdf::Object::Dictionary(pages));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.compress();
        doc.save(output_path)
            .map_err(|e| ExportError::SerializationError(format!("Error guardando PDF: {e}")))?;

        Ok(())
    }

    fn format_name(&self) -> &str {
        "PDF Reconstruido"
    }
}

fn agregar_elemento_pdf(
    job: &Job,
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    indice_elemento: usize,
    doc: &mut lopdf::Document,
    recursos_xobject: &mut lopdf::Dictionary,
    operaciones: &mut Vec<lopdf::content::Operation>,
) -> Result<(), ExportError> {
    if debe_preservarse_como_imagen_en_pdf(elemento)
        && agregar_recorte_pdf(
            job,
            pagina.dimensions.height,
            pagina.number,
            &elemento.bounding_box,
            doc,
            recursos_xobject,
            operaciones,
        )?
    {
        return Ok(());
    }

    match elemento.role {
        ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
            agregar_imagen_pdf(
                job,
                pagina,
                elemento,
                indice_elemento,
                doc,
                recursos_xobject,
                operaciones,
            )?;
        }
        ElementRole::Table => {
            if let Some(ref tabla) = elemento.table {
                agregar_tabla_pdf(pagina, elemento, tabla, operaciones);
            } else {
                agregar_texto_pdf(pagina, elemento, operaciones);
            }
        }
        ElementRole::Separator => agregar_separador_pdf(pagina, elemento, operaciones),
        _ => agregar_texto_pdf(pagina, elemento, operaciones),
    }

    Ok(())
}

fn agregar_imagen_pdf(
    job: &Job,
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    _indice_elemento: usize,
    doc: &mut lopdf::Document,
    recursos_xobject: &mut lopdf::Dictionary,
    operaciones: &mut Vec<lopdf::content::Operation>,
) -> Result<(), ExportError> {
    let Some(ref imagen) = elemento.image_crop else {
        return Ok(());
    };

    let _ = agregar_recorte_pdf(
        job,
        pagina.dimensions.height,
        imagen.page_number,
        &imagen.bounding_box,
        doc,
        recursos_xobject,
        operaciones,
    )?;
    Ok(())
}

fn debe_preservarse_como_imagen_en_pdf(elemento: &ElementBlueprint) -> bool {
    matches!(
        elemento.role,
        ElementRole::Title
            | ElementRole::Paragraph
            | ElementRole::Table
            | ElementRole::Formula
            | ElementRole::ListItem
            | ElementRole::Unknown
    ) && elemento
        .ocr_confidence
        .is_some_and(|valor| valor < PDF_FALLBACK_OCR_CONFIDENCE_THRESHOLD)
}

fn agregar_recorte_pdf(
    job: &Job,
    altura_pagina_px: u32,
    numero_pagina: u32,
    bounding_box: &Rectangle,
    doc: &mut lopdf::Document,
    recursos_xobject: &mut lopdf::Dictionary,
    operaciones: &mut Vec<lopdf::content::Operation>,
) -> Result<bool, ExportError> {
    let bytes = match recortar_imagen_desde_referencia(job, numero_pagina, bounding_box) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(false),
    };

    let (xobject_id, nombre) = crear_xobject_imagen_pdf(doc, &bytes).map_err(|e| {
        ExportError::SerializationError(format!("No se pudo crear XObject PDF: {e}"))
    })?;
    recursos_xobject.set(nombre.clone(), xobject_id);

    let x_pt = px_a_pt(bounding_box.x);
    let y_pt = px_a_pt(bounding_box.y);
    let width_pt = px_a_pt(bounding_box.width);
    let height_pt = px_a_pt(bounding_box.height);
    let origin_y = px_a_pt(altura_pagina_px) - y_pt - height_pt;

    operaciones.push(lopdf::content::Operation::new("q", vec![]));
    operaciones.push(lopdf::content::Operation::new(
        "cm",
        vec![
            lopdf::Object::Real(width_pt as f32),
            0.into(),
            0.into(),
            lopdf::Object::Real(height_pt as f32),
            lopdf::Object::Real(x_pt as f32),
            lopdf::Object::Real(origin_y as f32),
        ],
    ));
    operaciones.push(lopdf::content::Operation::new(
        "Do",
        vec![lopdf::Object::Name(nombre.into_bytes())],
    ));
    operaciones.push(lopdf::content::Operation::new("Q", vec![]));
    Ok(true)
}

fn agregar_texto_pdf(
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    operaciones: &mut Vec<lopdf::content::Operation>,
) {
    agregar_texto_pdf_con_modo(pagina, elemento, &elemento.text, operaciones, false);
}

fn agregar_texto_pdf_invisible(
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    operaciones: &mut Vec<lopdf::content::Operation>,
) {
    let texto = if elemento.role == ElementRole::Table {
        elemento
            .table
            .as_ref()
            .map(TableStructure::to_plain_text)
            .unwrap_or_else(|| elemento.text.clone())
    } else {
        elemento.text.clone()
    };

    agregar_texto_pdf_con_modo(pagina, elemento, &texto, operaciones, true);
}

fn agregar_texto_pdf_con_modo(
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    texto: &str,
    operaciones: &mut Vec<lopdf::content::Operation>,
    invisible: bool,
) {
    if texto.trim().is_empty() {
        return;
    }

    let width_pt = px_a_pt(elemento.bounding_box.width).max(12.0);
    let height_pt = px_a_pt(elemento.bounding_box.height).max(10.0);
    let x_pt = px_a_pt(elemento.bounding_box.x);
    let y_pt = px_a_pt(elemento.bounding_box.y);
    let pagina_alto_pt = px_a_pt(pagina.dimensions.height);
    let font_size = (height_pt * FACTOR_TAMANO_FUENTE)
        .max(TAMANO_FUENTE_MINIMO_PT)
        .min(TAMANO_FUENTE_MAXIMO_PT);
    let line_height = (font_size * 1.18).max(8.0);
    let ancho_util = (width_pt - 4.0).max(10.0);
    let lineas = envolver_texto_para_pdf(texto, ancho_util, font_size);

    for (indice_linea, linea) in lineas.iter().enumerate() {
        let baseline_y = pagina_alto_pt - y_pt - font_size - (indice_linea as f64 * line_height);
        if baseline_y <= 0.0 {
            break;
        }

        let ancho_estimado = estimar_ancho_linea_pdf(linea, font_size);
        let margen_izquierdo = elemento.style.left_indent_pt as f64;
        let texto_x = match elemento.style.alignment {
            AlignmentHint::Center => x_pt + ((width_pt - ancho_estimado) / 2.0).max(0.0),
            AlignmentHint::Right => x_pt + (width_pt - ancho_estimado - 2.0).max(0.0),
            AlignmentHint::Left | AlignmentHint::FullWidth => x_pt + margen_izquierdo + 1.0,
        };

        operaciones.push(lopdf::content::Operation::new("BT", vec![]));
        operaciones.push(lopdf::content::Operation::new(
            "Tf",
            vec!["F1".into(), lopdf::Object::Real(font_size as f32)],
        ));
        if invisible {
            operaciones.push(lopdf::content::Operation::new("Tr", vec![3.into()]));
        }
        operaciones.push(lopdf::content::Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                lopdf::Object::Real(texto_x as f32),
                lopdf::Object::Real(baseline_y as f32),
            ],
        ));
        operaciones.push(lopdf::content::Operation::new(
            "Tj",
            vec![objeto_texto_pdf_helvetica(linea)],
        ));
        operaciones.push(lopdf::content::Operation::new("ET", vec![]));
    }
}

fn agregar_fondo_pagina_visual_pdf(
    job: &Job,
    numero_pagina: u32,
    ancho_pagina_px: u32,
    altura_pagina_px: u32,
    doc: &mut lopdf::Document,
    recursos_xobject: &mut lopdf::Dictionary,
    operaciones: &mut Vec<lopdf::content::Operation>,
) -> Result<bool, ExportError> {
    let pagina = obtener_pagina(job, numero_pagina)?;
    let Some(bytes) = pagina.image_data.as_ref() else {
        return Ok(false);
    };

    let (xobject_id, nombre) = crear_xobject_imagen_pdf(doc, bytes).map_err(|e| {
        ExportError::SerializationError(format!("No se pudo crear XObject PDF: {e}"))
    })?;
    recursos_xobject.set(nombre.clone(), xobject_id);

    let width_pt = px_a_pt(ancho_pagina_px);
    let height_pt = px_a_pt(altura_pagina_px);
    operaciones.push(lopdf::content::Operation::new("q", vec![]));
    operaciones.push(lopdf::content::Operation::new(
        "cm",
        vec![
            lopdf::Object::Real(width_pt as f32),
            0.into(),
            0.into(),
            lopdf::Object::Real(height_pt as f32),
            0.into(),
            0.into(),
        ],
    ));
    operaciones.push(lopdf::content::Operation::new(
        "Do",
        vec![lopdf::Object::Name(nombre.into_bytes())],
    ));
    operaciones.push(lopdf::content::Operation::new("Q", vec![]));
    Ok(true)
}

fn agregar_tabla_pdf(
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    tabla: &TableStructure,
    operaciones: &mut Vec<lopdf::content::Operation>,
) {
    let filas = tabla.rows.len().max(1) as f64;
    let columnas = tabla.num_cols.max(1) as f64;
    let x_pt = px_a_pt(elemento.bounding_box.x);
    let y_pt = px_a_pt(elemento.bounding_box.y);
    let width_pt = px_a_pt(elemento.bounding_box.width).max(24.0);
    let height_pt = px_a_pt(elemento.bounding_box.height).max(24.0);
    let pagina_alto_pt = px_a_pt(pagina.dimensions.height);
    let celda_ancho = width_pt / columnas;
    let celda_alto = height_pt / filas;
    let base_y = pagina_alto_pt - y_pt - height_pt;

    operaciones.push(lopdf::content::Operation::new("q", vec![]));
    operaciones.push(lopdf::content::Operation::new("w", vec![0.8.into()]));

    for columna in 0..=columnas as usize {
        let x = x_pt + (columna as f64 * celda_ancho);
        operaciones.push(lopdf::content::Operation::new(
            "m",
            vec![
                lopdf::Object::Real(x as f32),
                lopdf::Object::Real(base_y as f32),
            ],
        ));
        operaciones.push(lopdf::content::Operation::new(
            "l",
            vec![
                lopdf::Object::Real(x as f32),
                lopdf::Object::Real((base_y + height_pt) as f32),
            ],
        ));
        operaciones.push(lopdf::content::Operation::new("S", vec![]));
    }

    for fila in 0..=filas as usize {
        let y = base_y + (fila as f64 * celda_alto);
        operaciones.push(lopdf::content::Operation::new(
            "m",
            vec![
                lopdf::Object::Real(x_pt as f32),
                lopdf::Object::Real(y as f32),
            ],
        ));
        operaciones.push(lopdf::content::Operation::new(
            "l",
            vec![
                lopdf::Object::Real((x_pt + width_pt) as f32),
                lopdf::Object::Real(y as f32),
            ],
        ));
        operaciones.push(lopdf::content::Operation::new("S", vec![]));
    }

    operaciones.push(lopdf::content::Operation::new("Q", vec![]));

    for (indice_fila, fila) in tabla.rows.iter().enumerate() {
        for (indice_columna, celda) in fila.iter().enumerate() {
            let mut estilo_celda = elemento.style.clone();
            if let Some(ref style) = celda.style {
                estilo_celda.alignment = alignment_hint_from_table(style.alignment);
                if style.is_emphasized {
                    estilo_celda.emphasis = EmphasisHint::Strong;
                }
            } else if tabla.is_header_row(indice_fila) {
                estilo_celda.emphasis = EmphasisHint::Strong;
                estilo_celda.alignment = AlignmentHint::Center;
            }

            let caja = ElementBlueprint {
                role: ElementRole::Paragraph,
                bounding_box: Rectangle {
                    x: elemento.bounding_box.x
                        + ((indice_columna as f64 * elemento.bounding_box.width as f64 / columnas)
                            .round() as u32)
                        + 4,
                    y: elemento.bounding_box.y
                        + ((indice_fila as f64 * elemento.bounding_box.height as f64 / filas)
                            .round() as u32)
                        + 4,
                    width: (celda_ancho.max(8.0) as u32).saturating_sub(8),
                    height: (celda_alto.max(8.0) as u32).saturating_sub(8),
                },
                reading_order: 0,
                column_index: 0,
                total_columns: 1,
                text: celda.content.clone(),
                ocr_confidence: elemento.ocr_confidence,
                layout_confidence: elemento.layout_confidence,
                suspected_header: false,
                suspected_footer: false,
                table: None,
                image_crop: None,
                style: estilo_celda,
            };
            agregar_texto_pdf(pagina, &caja, operaciones);
        }
    }
}

fn agregar_separador_pdf(
    pagina: &PageBlueprint,
    elemento: &ElementBlueprint,
    operaciones: &mut Vec<lopdf::content::Operation>,
) {
    let x_pt = px_a_pt(elemento.bounding_box.x);
    let y_pt = px_a_pt(elemento.bounding_box.y);
    let width_pt = px_a_pt(elemento.bounding_box.width);
    let pagina_alto_pt = px_a_pt(pagina.dimensions.height);
    let y = pagina_alto_pt - y_pt - 1.0;

    operaciones.push(lopdf::content::Operation::new("q", vec![]));
    operaciones.push(lopdf::content::Operation::new("w", vec![1.into()]));
    operaciones.push(lopdf::content::Operation::new(
        "m",
        vec![
            lopdf::Object::Real(x_pt as f32),
            lopdf::Object::Real(y as f32),
        ],
    ));
    operaciones.push(lopdf::content::Operation::new(
        "l",
        vec![
            lopdf::Object::Real((x_pt + width_pt) as f32),
            lopdf::Object::Real(y as f32),
        ],
    ));
    operaciones.push(lopdf::content::Operation::new("S", vec![]));
    operaciones.push(lopdf::content::Operation::new("Q", vec![]));
}

fn envolver_texto_para_pdf(texto: &str, ancho_pt: f64, font_size: f64) -> Vec<String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Vec::new();
    }

    let mut lineas = Vec::new();
    for parrafo in texto.lines() {
        let mut actual = String::new();
        for palabra in parrafo.split_whitespace() {
            let candidato = if actual.is_empty() {
                palabra.to_string()
            } else {
                format!("{actual} {palabra}")
            };

            if estimar_ancho_linea_pdf(&candidato, font_size) <= ancho_pt || actual.is_empty() {
                actual = candidato;
            } else {
                lineas.push(actual);
                actual = palabra.to_string();
            }
        }

        if !actual.is_empty() {
            lineas.push(actual);
        }
        if parrafo.is_empty() {
            lineas.push(String::new());
        }
    }

    lineas
}

fn estimar_ancho_linea_pdf(texto: &str, font_size: f64) -> f64 {
    texto.chars().map(ancho_glifo_helvetica).sum::<f64>() * (font_size / HELVETICA_UNIDADES_POR_EM)
}

fn ancho_glifo_helvetica(caracter: char) -> f64 {
    let codigo = caracter as u32;
    if (32..=126).contains(&codigo) {
        let indice = (codigo - 32) as usize;
        HELVETICA_GLYPH_WIDTHS_ASCII[indice] as f64
    } else {
        HELVETICA_GLYPH_WIDTH_FALLBACK as f64
    }
}

fn objeto_texto_pdf_helvetica(texto: &str) -> lopdf::Object {
    lopdf::Object::String(
        codificar_texto_pdf_helvetica(texto),
        lopdf::StringFormat::Hexadecimal,
    )
}

fn codificar_texto_pdf_helvetica(texto: &str) -> Vec<u8> {
    texto
        .chars()
        .flat_map(codificar_caracter_pdf_helvetica)
        .collect()
}

fn codificar_caracter_pdf_helvetica(caracter: char) -> Vec<u8> {
    let original = caracter.to_string();
    if let Some(bytes) = codificar_winansi_sin_reemplazo(&original) {
        return bytes;
    }

    let ascii_aproximado = original
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>();
    if let Some(bytes) = codificar_winansi_sin_reemplazo(&ascii_aproximado) {
        return bytes;
    }

    vec![b'?']
}

fn codificar_winansi_sin_reemplazo(texto: &str) -> Option<Vec<u8>> {
    let (bytes, _, tuvo_reemplazos) = WINDOWS_1252.encode(texto);
    if tuvo_reemplazos {
        None
    } else {
        Some(bytes.into_owned())
    }
}

fn crear_xobject_imagen_pdf(
    doc: &mut lopdf::Document,
    datos_imagen: &[u8],
) -> Result<(lopdf::ObjectId, String), String> {
    let imagen_dyn =
        image::load_from_memory(datos_imagen).map_err(|e| format!("Decodificacion: {e}"))?;
    let rgb = imagen_dyn.to_rgb8();
    let (ancho, alto) = rgb.dimensions();
    let mut jpeg = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, PDF_IMAGE_JPEG_QUALITY)
        .encode(&rgb, ancho, alto, image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("Codificacion JPEG: {e}"))?;

    let img_dict = lopdf::dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => ancho as i64,
        "Height" => alto as i64,
        "ColorSpace" => "DeviceRGB",
        "BitsPerComponent" => 8,
        "Filter" => "DCTDecode",
    };

    let img_stream = lopdf::Stream::new(img_dict, jpeg.into_inner());
    let xobj_id = doc.add_object(img_stream);
    Ok((xobj_id, format!("Im{}", xobj_id.0)))
}

fn alignment_hint_from_table(alignment: TableCellAlignment) -> AlignmentHint {
    match alignment {
        TableCellAlignment::Left => AlignmentHint::Left,
        TableCellAlignment::Center => AlignmentHint::Center,
        TableCellAlignment::Right => AlignmentHint::Right,
    }
}

#[cfg(test)]
mod pdf_metric_tests {
    use super::{envolver_texto_para_pdf, estimar_ancho_linea_pdf};

    #[test]
    fn test_pdf_glyph_metrics_distingue_glifos_estrechos_y_anchos() {
        let ancho_estrecho = estimar_ancho_linea_pdf("iiiii", 12.0);
        let ancho_ancho = estimar_ancho_linea_pdf("WWWWW", 12.0);

        assert!(
            ancho_ancho > ancho_estrecho,
            "Helvetica debe estimar mas ancho para W que para i"
        );
    }

    #[test]
    fn test_pdf_wrapping_usa_metricas_reales_de_glifos() {
        let ancho_referencia = estimar_ancho_linea_pdf("iiiii iiiii", 12.0) + 0.5;
        let lineas_estrechas = envolver_texto_para_pdf("iiiii iiiii", ancho_referencia, 12.0);
        let lineas_anchas = envolver_texto_para_pdf("WWWWW WWWWW", ancho_referencia, 12.0);

        assert_eq!(
            lineas_estrechas.len(),
            1,
            "Dos palabras estrechas deben caber en el ancho de referencia"
        );
        assert_eq!(
            lineas_anchas.len(),
            2,
            "El mismo ancho no debe acomodar dos palabras anchas"
        );
    }
}
