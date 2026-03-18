use crate::domain::errors::ExportError;
use crate::domain::{
    AlignmentHint, DocumentBlueprint, ElementBlueprint, ElementRole, EmphasisHint, Job,
    OutputFormat, Page, Rectangle, TableStructure,
};
use crate::infrastructure::document_blueprints::HighFidelityBlueprintBuilder;
use crate::interfaces::ports::{DocumentBlueprintBuilderPort, ExporterPort, JobExporterPort};
use encoding_rs::WINDOWS_1252;
use lopdf::dictionary;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Alias público del puerto de exportación para compatibilidad histórica.
pub use crate::interfaces::ports::ExporterPort as Exporter;

/// DPI asumido para convertir geometría raster a tamaños tipográficos.
const DPI_REFERENCIA: f64 = 150.0;
/// Factor de conversión: 1 punto PDF = 1/72 pulgadas.
const PUNTOS_POR_PULGADA: f64 = 72.0;
/// Fracción de alto del bounding box usada como tamaño de fuente PDF.
const FACTOR_TAMANO_FUENTE: f64 = 0.8;
/// Tamaño mínimo de fuente PDF.
const TAMANO_FUENTE_MINIMO_PT: f64 = 6.0;
/// Tamaño máximo de fuente PDF.
const TAMANO_FUENTE_MAXIMO_PT: f64 = 72.0;
/// Conversión aproximada de punto tipográfico a EMUs en DOCX.
const EMUS_POR_PUNTO: u64 = 12_700;
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

/// Registro por defecto que resuelve exportadores concretos según `OutputFormat`.
///
/// La resolución vive en infraestructura para que la aplicación no dependa de
/// constructores concretos ni replique el `match` en múltiples sitios.
pub struct DefaultJobExporter {
    txt: Arc<dyn ExporterPort>,
    docx: Arc<dyn ExporterPort>,
    latex: Arc<dyn ExporterPort>,
    pdf: Arc<dyn ExporterPort>,
    json: Arc<dyn ExporterPort>,
}

impl DefaultJobExporter {
    /// Construye el registro con los exportadores integrados del producto.
    pub fn new() -> Self {
        Self {
            txt: Arc::new(TxtExporter::new()),
            docx: Arc::new(DocxExporter::new()),
            latex: Arc::new(LatexExporter::new()),
            pdf: Arc::new(PdfReconstructedExporter::new()),
            json: Arc::new(JsonExporter::new()),
        }
    }

    /// Resuelve el exportador concreto para el formato indicado.
    fn exportador_para(&self, formato: OutputFormat) -> &dyn ExporterPort {
        match formato {
            OutputFormat::Txt => self.txt.as_ref(),
            OutputFormat::Docx => self.docx.as_ref(),
            OutputFormat::Latex => self.latex.as_ref(),
            OutputFormat::Pdf => self.pdf.as_ref(),
            OutputFormat::Json => self.json.as_ref(),
        }
    }
}

impl Default for DefaultJobExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JobExporterPort for DefaultJobExporter {
    fn export_job(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        self.exportador_para(job.formato_salida)
            .export(job, output_path)
    }
}

/// Exportador de documentos OCR a texto plano legible por humanos.
pub struct TxtExporter;

impl TxtExporter {
    /// Construye un exportador TXT sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TxtExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for TxtExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;

        let mut contenido = String::new();
        contenido.push_str(&format!("Documento: {}\n", job.document.id));
        contenido.push_str(&format!(
            "Archivo fuente: {}\n",
            job.document.source_path.display()
        ));
        contenido.push_str(&format!("Perfil: {:?}\n", job.profile));
        contenido.push_str(&format!("Estado: {:?}\n\n", job.status));

        let blueprint = construir_blueprint(&job.document)?;

        for pagina in &blueprint.pages {
            contenido.push_str(&format!("===== PAGINA {} =====\n\n", pagina.number));

            for elemento in &pagina.elements {
                match elemento.role {
                    ElementRole::Title => {
                        contenido.push_str(&format!("{}\n\n", elemento.text.to_uppercase()));
                    }
                    ElementRole::Paragraph | ElementRole::ListItem => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                    ElementRole::Table => {
                        if let Some(ref tabla) = elemento.table {
                            let tabla_txt = tabla.to_plain_text();
                            if !tabla_txt.is_empty() {
                                contenido.push_str(&tabla_txt);
                                contenido.push('\n');
                            }
                        } else if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        } else {
                            contenido.push_str("[Tabla sin contenido]\n\n");
                        }
                    }
                    ElementRole::Formula => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str("Formula: ");
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                    ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
                        contenido.push_str("[Activo visual preservado en exportadores ricos]\n\n");
                    }
                    ElementRole::Separator | ElementRole::Unknown => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                }
            }
        }

        fs::write(output_path, contenido)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "TXT"
    }
}

/// Exportador a DOCX editable guiado por el blueprint visual.
///
/// Esta implementación prioriza compatibilidad amplia con Word y una
/// reconstrucción razonable de títulos, tablas, imágenes y jerarquía visual.
/// No intenta aún posicionamiento absoluto agresivo; para ese caso, LaTeX y PDF
/// siguen siendo rutas más fieles visualmente.
pub struct DocxExporter;

impl DocxExporter {
    /// Construye un exportador DOCX sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl Default for DocxExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for DocxExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;

        let blueprint = construir_blueprint(&job.document)?;
        let mut relaciones_imagen = Vec::new();
        let mut media = Vec::new();
        let mut cuerpo = String::new();
        let mut contador_rel = 1usize;
        let mut contador_docpr = 1u32;

        for (indice_pagina, pagina) in blueprint.pages.iter().enumerate() {
            cuerpo.push_str(&docx_xml_pagina(
                job,
                pagina,
                &mut relaciones_imagen,
                &mut media,
                &mut contador_rel,
                &mut contador_docpr,
            )?);

            if indice_pagina + 1 < blueprint.pages.len() {
                cuerpo.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
            }
        }

        let primera_pagina = blueprint
            .pages
            .first()
            .ok_or_else(|| ExportError::SerializationError("Documento sin páginas".to_string()))?;
        let ancho_twips = pt_a_twips(px_a_pt(primera_pagina.dimensions.width)) as u64;
        let alto_twips = pt_a_twips(px_a_pt(primera_pagina.dimensions.height)) as u64;

        let document_xml = format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
                "<w:document xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" ",
                "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ",
                "xmlns:o=\"urn:schemas-microsoft-com:office:office\" ",
                "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
                "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" ",
                "xmlns:v=\"urn:schemas-microsoft-com:vml\" ",
                "xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\" ",
                "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
                "xmlns:w10=\"urn:schemas-microsoft-com:office:word\" ",
                "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
                "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" ",
                "xmlns:wpg=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\" ",
                "xmlns:wpi=\"http://schemas.microsoft.com/office/word/2010/wordprocessingInk\" ",
                "xmlns:wne=\"http://schemas.microsoft.com/office/word/2006/wordml\" ",
                "xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" ",
                "mc:Ignorable=\"w14 wp14\">",
                "<w:body>{}<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/>",
                "<w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\" ",
                "w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
            ),
            cuerpo, ancho_twips, alto_twips
        );

        let document_rels = construir_document_rels(&relaciones_imagen);
        let content_types = construir_content_types_docx(!media.is_empty());
        let root_rels = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
            "</Relationships>"
        );

        let mut entradas = vec![
            ZipEntry::new("[Content_Types].xml", content_types.into_bytes()),
            ZipEntry::new("_rels/.rels", root_rels.as_bytes().to_vec()),
            ZipEntry::new("word/document.xml", document_xml.into_bytes()),
            ZipEntry::new("word/_rels/document.xml.rels", document_rels.into_bytes()),
        ];

        for (nombre, bytes) in media {
            entradas.push(ZipEntry::new(format!("word/media/{nombre}"), bytes));
        }

        escribir_zip_sin_compresion(&entradas, output_path)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "DOCX"
    }
}

/// Exportador LaTeX con posicionamiento guiado por geometría OCR.
///
/// La ruta LaTeX prioriza fidelidad visual sobre pureza semántica: usa bloques
/// absolutos cuando el blueprint indica que la posición importa.
pub struct LatexExporter;

impl LatexExporter {
    /// Construye un exportador LaTeX sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LatexExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for LatexExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;

        let blueprint = construir_blueprint(&job.document)?;
        let primera_pagina = blueprint
            .pages
            .first()
            .ok_or_else(|| ExportError::SerializationError("Documento sin páginas".to_string()))?;

        let directorio_assets = directorio_assets(output_path);
        fs::create_dir_all(&directorio_assets)?;

        let mut contenido = String::new();
        contenido.push_str("\\documentclass{article}\n");
        contenido.push_str(&format!(
            "\\usepackage[paperwidth={:.2}pt,paperheight={:.2}pt,margin=0pt]{{geometry}}\n",
            px_a_pt(primera_pagina.dimensions.width),
            px_a_pt(primera_pagina.dimensions.height)
        ));
        contenido.push_str("\\usepackage[absolute,overlay]{textpos}\n");
        contenido.push_str("\\usepackage{graphicx}\n");
        contenido.push_str("\\usepackage{array}\n");
        contenido.push_str("\\usepackage{longtable}\n");
        contenido.push_str("\\usepackage{ragged2e}\n");
        contenido.push_str("\\pagestyle{empty}\n");
        contenido.push_str("\\setlength{\\TPHorizModule}{1pt}\n");
        contenido.push_str("\\setlength{\\TPVertModule}{1pt}\n");
        contenido.push_str("\\begin{document}\n");
        contenido.push_str("\\setlength{\\parindent}{0pt}\n");

        for (indice_pagina, pagina) in blueprint.pages.iter().enumerate() {
            for (indice_elemento, elemento) in pagina.elements.iter().enumerate() {
                let nombre_asset =
                    format!("page{}_element{}.png", pagina.number, indice_elemento + 1);
                contenido.push_str(&latex_elemento(
                    job,
                    pagina,
                    elemento,
                    &directorio_assets,
                    &nombre_asset,
                )?);
            }

            if indice_pagina + 1 < blueprint.pages.len() {
                contenido.push_str("\\newpage\n");
            }
        }

        contenido.push_str("\\end{document}\n");
        fs::write(output_path, contenido)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "LaTeX"
    }
}

/// Exportador a PDF reconstruido a partir del blueprint visual.
///
/// Esta ruta renderiza texto, tablas, líneas e imágenes directamente sobre una
/// página PDF en blanco. El resultado deja de depender de incrustar el escaneo
/// completo y se acerca más a un documento reeditable/facsímil generado.
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
                resources_dict.set("XObject", Object::Dictionary(recursos_xobject));
            }

            let resources_id = doc.add_object(Object::Dictionary(resources_dict));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![
                    0.into(),
                    0.into(),
                    Object::Real(ancho_pt as f32),
                    Object::Real(alto_pt as f32),
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

/// Exportador a JSON estructurado para integración y depuración.
pub struct JsonExporter;

impl JsonExporter {
    /// Crea un nuevo exportador JSON.
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for JsonExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;
        let json_content = serde_json::to_string_pretty(job)
            .map_err(|e| ExportError::SerializationError(e.to_string()))?;
        fs::write(output_path, json_content)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "JSON"
    }
}

fn construir_blueprint(
    documento: &crate::domain::Document,
) -> Result<DocumentBlueprint, ExportError> {
    HighFidelityBlueprintBuilder::new()
        .build_blueprint(documento)
        .map_err(|e| {
            ExportError::SerializationError(format!("No se pudo construir blueprint: {e}"))
        })
}

fn asegurar_directorio_padre(ruta: &Path) -> Result<(), ExportError> {
    if let Some(parent) = ruta.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn directorio_assets(ruta_salida: &Path) -> PathBuf {
    let stem = ruta_salida
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("documento");
    ruta_salida.with_file_name(format!("{stem}_assets"))
}

fn px_a_pt(px: u32) -> f64 {
    (px as f64) * (PUNTOS_POR_PULGADA / DPI_REFERENCIA)
}

fn pt_a_twips(pt: f64) -> u32 {
    (pt * 20.0).round().max(0.0) as u32
}

fn obtener_pagina<'a>(job: &'a Job, numero_pagina: u32) -> Result<&'a Page, ExportError> {
    job.document
        .pages
        .iter()
        .find(|pagina| pagina.number == numero_pagina)
        .ok_or_else(|| {
            ExportError::SerializationError(format!(
                "No existe la pagina {numero_pagina} en el documento"
            ))
        })
}

fn recortar_imagen_desde_referencia(
    job: &Job,
    numero_pagina: u32,
    bounding_box: &Rectangle,
) -> Result<Vec<u8>, ExportError> {
    let pagina = obtener_pagina(job, numero_pagina)?;
    let datos = pagina.image_data.as_ref().ok_or_else(|| {
        ExportError::SerializationError(format!(
            "La pagina {numero_pagina} no conserva raster en memoria"
        ))
    })?;

    let imagen = image::load_from_memory(datos).map_err(|e| {
        ExportError::SerializationError(format!("No se pudo decodificar raster: {e}"))
    })?;

    let ancho = imagen.width();
    let alto = imagen.height();
    let x = bounding_box.x.min(ancho);
    let y = bounding_box.y.min(alto);
    let w = bounding_box.width.min(ancho.saturating_sub(x)).max(1);
    let h = bounding_box.height.min(alto.saturating_sub(y)).max(1);

    let recorte = imagen.crop_imm(x, y, w, h);
    let mut cursor = Cursor::new(Vec::new());
    recorte
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| {
            ExportError::SerializationError(format!("No se pudo codificar recorte PNG: {e}"))
        })?;

    Ok(cursor.into_inner())
}

fn agregar_elemento_pdf(
    job: &Job,
    pagina: &crate::domain::PageBlueprint,
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
    pagina: &crate::domain::PageBlueprint,
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
    pagina: &crate::domain::PageBlueprint,
    elemento: &ElementBlueprint,
    operaciones: &mut Vec<lopdf::content::Operation>,
) {
    if elemento.text.trim().is_empty() {
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
    let lineas = envolver_texto_para_pdf(&elemento.text, ancho_util, font_size);

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

fn agregar_tabla_pdf(
    pagina: &crate::domain::PageBlueprint,
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
                style: elemento.style.clone(),
            };
            agregar_texto_pdf(pagina, &caja, operaciones);
        }
    }
}

fn agregar_separador_pdf(
    pagina: &crate::domain::PageBlueprint,
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

fn latex_elemento(
    job: &Job,
    _pagina: &crate::domain::PageBlueprint,
    elemento: &ElementBlueprint,
    directorio_assets: &Path,
    nombre_asset: &str,
) -> Result<String, ExportError> {
    let x = px_a_pt(elemento.bounding_box.x);
    let y = px_a_pt(elemento.bounding_box.y);
    let width = px_a_pt(elemento.bounding_box.width);
    let height = px_a_pt(elemento.bounding_box.height);
    let mut contenido = String::new();

    contenido.push_str(&format!(
        "\\begin{{textblock*}}{{{width:.2}pt}}({x:.2}pt,{y:.2}pt)\n"
    ));

    match elemento.role {
        ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
            if let Some(ref imagen) = elemento.image_crop {
                match recortar_imagen_desde_referencia(
                    job,
                    imagen.page_number,
                    &imagen.bounding_box,
                ) {
                    Ok(bytes) => {
                        let ruta = directorio_assets.join(nombre_asset);
                        fs::write(&ruta, bytes)?;
                        let nombre_relativo = ruta
                            .file_name()
                            .and_then(|valor| valor.to_str())
                            .unwrap_or(nombre_asset);
                        let directorio_relativo = directorio_assets
                            .file_name()
                            .and_then(|valor| valor.to_str())
                            .unwrap_or("documento_assets");
                        contenido.push_str(&format!(
                            "\\includegraphics[width={width:.2}pt,height={height:.2}pt]{{{}/{}}}\n",
                            escape_latex(directorio_relativo),
                            escape_latex(nombre_relativo)
                        ));
                    }
                    Err(_) => {
                        contenido.push_str("\\fbox{Imagen no disponible en memoria}\n");
                    }
                }
            }
        }
        ElementRole::Table => {
            if let Some(ref tabla) = elemento.table {
                contenido.push_str(&latex_tabla(tabla, width));
            } else if !elemento.text.trim().is_empty() {
                contenido.push_str(&latex_parrafo(
                    &elemento.text,
                    elemento.style.alignment,
                    elemento.style.emphasis,
                    elemento.style.font_scale,
                ));
            }
        }
        _ => {
            if !elemento.text.trim().is_empty() {
                contenido.push_str(&latex_parrafo(
                    &elemento.text,
                    elemento.style.alignment,
                    elemento.style.emphasis,
                    elemento.style.font_scale,
                ));
            }
        }
    }

    contenido.push_str("\\end{textblock*}\n");
    Ok(contenido)
}

fn latex_parrafo(
    texto: &str,
    alineacion: AlignmentHint,
    emphasis: EmphasisHint,
    escala_fuente: f32,
) -> String {
    let mut contenido = String::new();
    let tamano = (11.0 * escala_fuente as f64).clamp(9.0, 24.0);
    let interlineado = (tamano * 1.18).clamp(10.0, 28.0);
    contenido.push_str(&format!(
        "\\fontsize{{{tamano:.2}pt}}{{{interlineado:.2}pt}}\\selectfont\n"
    ));
    contenido.push_str(match alineacion {
        AlignmentHint::Center => "\\centering\n",
        AlignmentHint::Right => "\\raggedleft\n",
        AlignmentHint::Left | AlignmentHint::FullWidth => "\\RaggedRight\n",
    });

    let texto_escapado = escape_latex(texto);
    if emphasis == EmphasisHint::Strong {
        contenido.push_str(&format!("\\textbf{{{texto_escapado}}}\n"));
    } else {
        contenido.push_str(&texto_escapado);
        contenido.push('\n');
    }
    contenido
}

fn latex_tabla(tabla: &TableStructure, ancho_total_pt: f64) -> String {
    if tabla.rows.is_empty() || tabla.num_cols == 0 {
        return "[Tabla vacia]\n".to_string();
    }

    let columnas = tabla.num_cols.max(1) as usize;
    let ancho_columna = (ancho_total_pt / columnas as f64).max(48.0);
    let especificacion = (0..columnas)
        .map(|_| format!("|p{{{ancho_columna:.2}pt}}"))
        .collect::<String>()
        + "|";

    let mut contenido = String::new();
    contenido.push_str("\\renewcommand{\\arraystretch}{1.05}\n");
    contenido.push_str(&format!(
        "\\begin{{tabular}}{{{especificacion}}}\n\\hline\n"
    ));

    for fila in &tabla.rows {
        let celdas = fila
            .iter()
            .map(|celda| escape_latex(&celda.content))
            .collect::<Vec<_>>();
        contenido.push_str(&celdas.join(" & "));
        contenido.push_str(" \\\\\n\\hline\n");
    }

    contenido.push_str("\\end{tabular}\n");
    contenido
}

fn docx_xml_elemento(
    job: &Job,
    _pagina: &crate::domain::PageBlueprint,
    elemento: &ElementBlueprint,
    relaciones_imagen: &mut Vec<(String, String)>,
    media: &mut Vec<(String, Vec<u8>)>,
    contador_rel: &mut usize,
    contador_docpr: &mut u32,
) -> Result<String, ExportError> {
    match elemento.role {
        ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
            if let Some(ref imagen) = elemento.image_crop {
                match recortar_imagen_desde_referencia(
                    job,
                    imagen.page_number,
                    &imagen.bounding_box,
                ) {
                    Ok(bytes) => {
                        let rel_id = format!("rId{}", *contador_rel);
                        let nombre = format!("image{}.png", *contador_rel);
                        relaciones_imagen.push((rel_id.clone(), nombre.clone()));
                        media.push((nombre, bytes));
                        *contador_rel += 1;
                        let width_emu = (px_a_pt(elemento.bounding_box.width)
                            * EMUS_POR_PUNTO as f64)
                            .round()
                            .max(1.0) as u64;
                        let height_emu = (px_a_pt(elemento.bounding_box.height)
                            * EMUS_POR_PUNTO as f64)
                            .round()
                            .max(1.0) as u64;
                        let xml = docx_xml_imagen(
                            &rel_id,
                            width_emu,
                            height_emu,
                            *contador_docpr,
                            elemento.style.alignment,
                        );
                        *contador_docpr += 1;
                        Ok(xml)
                    }
                    Err(_) => Ok(docx_xml_parrafo(
                        "Imagen omitida: raster no disponible en memoria",
                        &elemento.style,
                    )),
                }
            } else {
                Ok(String::new())
            }
        }
        ElementRole::Table => {
            if let Some(ref tabla) = elemento.table {
                Ok(docx_xml_tabla(tabla))
            } else {
                Ok(docx_xml_parrafo(&elemento.text, &elemento.style))
            }
        }
        _ => Ok(docx_xml_parrafo(&elemento.text, &elemento.style)),
    }
}

fn docx_xml_pagina(
    job: &Job,
    pagina: &crate::domain::PageBlueprint,
    relaciones_imagen: &mut Vec<(String, String)>,
    media: &mut Vec<(String, Vec<u8>)>,
    contador_rel: &mut usize,
    contador_docpr: &mut u32,
) -> Result<String, ExportError> {
    let mut xml = String::new();
    let mut columnares: Vec<&ElementBlueprint> = Vec::new();

    for elemento in &pagina.elements {
        if elemento.total_columns == 2 {
            columnares.push(elemento);
            continue;
        }

        if !columnares.is_empty() {
            xml.push_str(&docx_xml_banda_columnas(
                job,
                pagina,
                &columnares,
                relaciones_imagen,
                media,
                contador_rel,
                contador_docpr,
            )?);
            columnares.clear();
        }

        xml.push_str(&docx_xml_elemento(
            job,
            pagina,
            elemento,
            relaciones_imagen,
            media,
            contador_rel,
            contador_docpr,
        )?);
    }

    if !columnares.is_empty() {
        xml.push_str(&docx_xml_banda_columnas(
            job,
            pagina,
            &columnares,
            relaciones_imagen,
            media,
            contador_rel,
            contador_docpr,
        )?);
    }

    Ok(xml)
}

fn docx_xml_banda_columnas(
    job: &Job,
    pagina: &crate::domain::PageBlueprint,
    elementos: &[&ElementBlueprint],
    relaciones_imagen: &mut Vec<(String, String)>,
    media: &mut Vec<(String, Vec<u8>)>,
    contador_rel: &mut usize,
    contador_docpr: &mut u32,
) -> Result<String, ExportError> {
    let mut izquierda = String::new();
    let mut derecha = String::new();

    for elemento in elementos {
        let xml = docx_xml_elemento(
            job,
            pagina,
            elemento,
            relaciones_imagen,
            media,
            contador_rel,
            contador_docpr,
        )?;
        if elemento.column_index == 0 {
            izquierda.push_str(&xml);
        } else {
            derecha.push_str(&xml);
        }
    }

    if izquierda.is_empty() {
        izquierda.push_str("<w:p/>");
    } else {
        izquierda.push_str("<w:p/>");
    }
    if derecha.is_empty() {
        derecha.push_str("<w:p/>");
    } else {
        derecha.push_str("<w:p/>");
    }

    Ok(format!(
        concat!(
            "<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"pct\"/>",
            "<w:tblDescription w:val=\"column-layout\"/>",
            "<w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/>",
            "<w:bottom w:val=\"nil\"/><w:right w:val=\"nil\"/>",
            "<w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders>",
            "</w:tblPr><w:tr>",
            "<w:tc><w:tcPr><w:tcW w:w=\"5000\" w:type=\"pct\"/></w:tcPr>{}</w:tc>",
            "<w:tc><w:tcPr><w:tcW w:w=\"5000\" w:type=\"pct\"/></w:tcPr>{}</w:tc>",
            "</w:tr></w:tbl>"
        ),
        izquierda, derecha
    ))
}

fn docx_xml_parrafo(texto: &str, estilo: &crate::domain::StyleHints) -> String {
    if texto.trim().is_empty() {
        return String::new();
    }

    let half_points = ((11.0 * estilo.font_scale as f64).clamp(9.0, 26.0) * 2.0).round() as u32;
    let alineacion_xml = match estilo.alignment {
        AlignmentHint::Left | AlignmentHint::FullWidth => "left",
        AlignmentHint::Center => "center",
        AlignmentHint::Right => "right",
    };
    let bold = if estilo.emphasis == EmphasisHint::Strong {
        "<w:b/>"
    } else {
        ""
    };
    let keep_next = if estilo.keep_with_next {
        "<w:keepNext/>"
    } else {
        ""
    };
    let spacing_before = pt_a_twips(estilo.spacing_before_pt as f64);
    let left_indent = pt_a_twips(estilo.left_indent_pt as f64);

    format!(
        concat!(
            "<w:p><w:pPr>{}<w:jc w:val=\"{}\"/><w:spacing w:before=\"{}\" w:after=\"80\"/>",
            "<w:ind w:left=\"{}\"/></w:pPr>",
            "<w:r><w:rPr>{}<w:sz w:val=\"{}\"/></w:rPr>",
            "<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>"
        ),
        keep_next,
        alineacion_xml,
        spacing_before,
        left_indent,
        bold,
        half_points,
        escape_xml(texto)
    )
}

fn docx_xml_tabla(tabla: &TableStructure) -> String {
    if tabla.rows.is_empty() {
        return "<w:p><w:r><w:t>Tabla vacia</w:t></w:r></w:p>".to_string();
    }

    let mut xml = String::new();
    xml.push_str(
        "<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"pct\"/>\
         <w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         </w:tblBorders></w:tblPr>",
    );

    for fila in &tabla.rows {
        xml.push_str("<w:tr>");
        for celda in fila {
            xml.push_str("<w:tc><w:p><w:r><w:t xml:space=\"preserve\">");
            xml.push_str(&escape_xml(&celda.content));
            xml.push_str("</w:t></w:r></w:p></w:tc>");
        }
        xml.push_str("</w:tr>");
    }

    xml.push_str("</w:tbl>");
    xml
}

fn docx_xml_imagen(
    rel_id: &str,
    width_emu: u64,
    height_emu: u64,
    doc_pr_id: u32,
    alineacion: AlignmentHint,
) -> String {
    let alineacion_xml = match alineacion {
        AlignmentHint::Center => "center",
        AlignmentHint::Right => "right",
        AlignmentHint::Left | AlignmentHint::FullWidth => "left",
    };

    format!(
        concat!(
            "<w:p><w:pPr><w:jc w:val=\"{}\"/></w:pPr><w:r><w:drawing>",
            "<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" ",
            "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">",
            "<wp:extent cx=\"{}\" cy=\"{}\"/><wp:docPr id=\"{}\" name=\"Imagen {}\"/>",
            "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
            "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"Imagen {}\"/><pic:cNvPicPr/></pic:nvPicPr>",
            "<pic:blipFill><a:blip r:embed=\"{}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"/>",
            "<a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
            "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>",
            "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ),
        alineacion_xml,
        width_emu,
        height_emu,
        doc_pr_id,
        doc_pr_id,
        doc_pr_id,
        rel_id,
        width_emu,
        height_emu
    )
}

fn construir_document_rels(relaciones_imagen: &[(String, String)]) -> String {
    let mut xml = String::new();
    xml.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );

    for (rel_id, nombre) in relaciones_imagen {
        xml.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>",
            rel_id, nombre
        ));
    }

    xml.push_str("</Relationships>");
    xml
}

fn construir_content_types_docx(tiene_png: bool) -> String {
    let mut xml = String::new();
    xml.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>",
    );
    if tiene_png {
        xml.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    xml.push_str(
        "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         </Types>",
    );
    xml
}

fn escape_xml(texto: &str) -> String {
    texto
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn escape_latex(texto: &str) -> String {
    texto
        .replace('\\', "\\textbackslash{}")
        .replace('&', "\\&")
        .replace('%', "\\%")
        .replace('$', "\\$")
        .replace('#', "\\#")
        .replace('_', "\\_")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('~', "\\textasciitilde{}")
        .replace('^', "\\textasciicircum{}")
}

struct ZipEntry {
    path: String,
    bytes: Vec<u8>,
}

impl ZipEntry {
    fn new(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            bytes,
        }
    }
}

fn escribir_zip_sin_compresion(
    entradas: &[ZipEntry],
    ruta_salida: &Path,
) -> Result<(), ExportError> {
    let mut archivo = Vec::new();
    let mut directorio_central = Vec::new();
    let mut desplazamiento = 0u32;

    for entrada in entradas {
        let nombre = entrada.path.as_bytes();
        let crc = crc32(&entrada.bytes);
        let tamano = entrada.bytes.len() as u32;

        escribir_u32(&mut archivo, 0x04034b50);
        escribir_u16(&mut archivo, 20);
        escribir_u16(&mut archivo, 0);
        escribir_u16(&mut archivo, 0);
        escribir_u16(&mut archivo, 0);
        escribir_u16(&mut archivo, 0);
        escribir_u32(&mut archivo, crc);
        escribir_u32(&mut archivo, tamano);
        escribir_u32(&mut archivo, tamano);
        escribir_u16(&mut archivo, nombre.len() as u16);
        escribir_u16(&mut archivo, 0);
        archivo.extend_from_slice(nombre);
        archivo.extend_from_slice(&entrada.bytes);

        escribir_u32(&mut directorio_central, 0x02014b50);
        escribir_u16(&mut directorio_central, 20);
        escribir_u16(&mut directorio_central, 20);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u32(&mut directorio_central, crc);
        escribir_u32(&mut directorio_central, tamano);
        escribir_u32(&mut directorio_central, tamano);
        escribir_u16(&mut directorio_central, nombre.len() as u16);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u16(&mut directorio_central, 0);
        escribir_u32(&mut directorio_central, 0);
        escribir_u32(&mut directorio_central, desplazamiento);
        directorio_central.extend_from_slice(nombre);

        desplazamiento = archivo.len() as u32;
    }

    let inicio_directorio = archivo.len() as u32;
    archivo.extend_from_slice(&directorio_central);

    escribir_u32(&mut archivo, 0x06054b50);
    escribir_u16(&mut archivo, 0);
    escribir_u16(&mut archivo, 0);
    escribir_u16(&mut archivo, entradas.len() as u16);
    escribir_u16(&mut archivo, entradas.len() as u16);
    escribir_u32(&mut archivo, directorio_central.len() as u32);
    escribir_u32(&mut archivo, inicio_directorio);
    escribir_u16(&mut archivo, 0);

    fs::write(ruta_salida, archivo)?;
    Ok(())
}

fn escribir_u16(buffer: &mut Vec<u8>, valor: u16) {
    buffer.extend_from_slice(&valor.to_le_bytes());
}

fn escribir_u32(buffer: &mut Vec<u8>, valor: u32) {
    buffer.extend_from_slice(&valor.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;

    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mascara = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mascara;
        }
    }

    !crc
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
