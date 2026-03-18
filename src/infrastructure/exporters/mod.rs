use crate::domain::errors::ExportError;
use crate::domain::{BlockType, Job};
use crate::interfaces::ports::ExporterPort;
use lopdf::dictionary;
use std::fs;
use std::path::Path;

/// Alias público del puerto de exportación para compatibilidad histórica.
pub use crate::interfaces::ports::ExporterPort as Exporter;

/// Exportador de documentos OCR a texto plano legible por humanos.
///
/// La implementación prioriza inspección y portabilidad: el resultado puede
/// abrirse en cualquier editor de texto y es útil para debugging, diffing y
/// revisión manual del OCR sin depender de herramientas especializadas.
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
        log::info!("Exportando trabajo {} a TXT: {:?}", job.id, output_path);

        let mut content = String::new();

        content.push_str(&format!("Documento: {}\n", job.document.id));
        content.push_str(&format!(
            "Archivo fuente: {}\n",
            job.document.source_path.display()
        ));
        content.push_str(&format!("Perfil: {:?}\n", job.profile));
        content.push_str(&format!("Estado: {:?}\n\n", job.status));

        for page in &job.document.pages {
            content.push_str(&format!("===== PAGINA {} =====\n\n", page.number));

            for block in &page.blocks {
                match block.block_type {
                    BlockType::Title => {
                        content.push_str(&format!("{}\n\n", block.content.to_uppercase()));
                    }
                    BlockType::Text | BlockType::List => {
                        content.push_str(&format!("{}\n\n", block.content));
                    }
                    BlockType::Table => {
                        if let Some(ref estructura) = block.table_structure {
                            let tabla_txt = estructura.to_plain_text();
                            if !tabla_txt.is_empty() {
                                content.push_str(&tabla_txt);
                                content.push('\n');
                            } else {
                                content.push_str("[Tabla vacia]\n\n");
                            }
                        } else if !block.content.is_empty() {
                            content.push_str(&format!("{}\n\n", block.content));
                        } else {
                            content.push_str("[Tabla sin contenido]\n\n");
                        }
                    }
                    BlockType::Formula => {
                        content.push_str(&format!("Formula: {}\n\n", block.content));
                    }
                    BlockType::Image => {
                        content.push_str("[Imagen preservada en formato visual]\n\n");
                    }
                    _ => {
                        if !block.content.trim().is_empty() {
                            content.push_str(&format!("{}\n\n", block.content));
                        }
                    }
                }
            }
        }

        fs::write(output_path, content)?;
        log::info!("Exportacion TXT completada");
        Ok(())
    }

    fn format_name(&self) -> &str {
        "TXT"
    }
}

/// Exportador a PDF sandwich con texto invisible seleccionable.
///
/// La estrategia conserva la imagen original como fondo y superpone una capa de
/// texto OCR en coordenadas PDF. Ese diseño preserva fidelidad visual mientras
/// habilita búsqueda y selección, que es el comportamiento esperado en flujos de
/// archivo y revisión documental.
///
/// # Trade-offs
///
/// Usar `lopdf` directamente reduce abstracciones y dependencia de motores PDF
/// pesados, pero obliga a controlar manualmente coordenadas y operadores.
pub struct PdfSandwichExporter;

/// DPI asumido para la conversion pixel -> puntos PDF.
/// 150 DPI es un valor conservador para documentos escaneados.
const DPI_REFERENCIA: f64 = 150.0;

/// Factor de conversion: 1 punto PDF = 1/72 pulgadas.
const PUNTOS_POR_PULGADA: f64 = 72.0;

/// Fraccion de la altura del bounding box usada como tamano de fuente estimado.
/// 0.8 deja un margen del 20% para evitar que el texto invisible desborde el bloque.
const FACTOR_TAMANO_FUENTE: f64 = 0.8;

/// Tamano minimo de fuente en puntos PDF para la capa de texto invisible.
/// Evita fuentes de tamano cero o negativo en bloques de altura despreciable.
const TAMANO_FUENTE_MINIMO_PT: f64 = 6.0;

/// Tamano maximo de fuente en puntos PDF para la capa de texto invisible.
/// Limita bloques de altura exagerada (ej: imagen de pagina completa).
const TAMANO_FUENTE_MAXIMO_PT: f64 = 72.0;

impl PdfSandwichExporter {
    /// Construye un exportador PDF sandwich sin estado mutable.
    pub fn new() -> Self {
        Self
    }

    /// Convierte pixeles a puntos PDF.
    fn px_a_pt(px: u32) -> f64 {
        (px as f64) * (PUNTOS_POR_PULGADA / DPI_REFERENCIA)
    }
}

impl Default for PdfSandwichExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for PdfSandwichExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object};

        log::info!(
            "Exportando trabajo {} a PDF Sandwich: {:?}",
            job.id,
            output_path
        );

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });

        let mut page_ids: Vec<Object> = Vec::new();

        for pagina in &job.document.pages {
            let ancho_pt = Self::px_a_pt(pagina.dimensions.width);
            let alto_pt = Self::px_a_pt(pagina.dimensions.height);

            let mut operaciones_imagen: Vec<Operation> = Vec::new();
            let mut recursos_xobject = lopdf::Dictionary::new();

            if let Some(ref datos_imagen) = pagina.image_data {
                match Self::crear_xobject_imagen(&mut doc, datos_imagen) {
                    Ok((xobj_id, img_name)) => {
                        recursos_xobject.set(img_name.clone(), xobj_id);

                        operaciones_imagen.push(Operation::new("q", vec![]));
                        operaciones_imagen.push(Operation::new(
                            "cm",
                            vec![
                                ancho_pt.into(),
                                0.into(),
                                0.into(),
                                alto_pt.into(),
                                0.into(),
                                0.into(),
                            ],
                        ));
                        operaciones_imagen.push(Operation::new(
                            "Do",
                            vec![Object::Name(img_name.into_bytes())],
                        ));
                        operaciones_imagen.push(Operation::new("Q", vec![]));
                    }
                    Err(e) => {
                        log::warn!(
                            "No se pudo crear XObject de imagen para pagina {}: {}",
                            pagina.number,
                            e
                        );
                    }
                }
            }

            let mut operaciones_texto: Vec<Operation> = Vec::new();
            let bloques_con_texto: Vec<_> = pagina
                .blocks
                .iter()
                .filter(|b| !b.content.trim().is_empty())
                .collect();

            if !bloques_con_texto.is_empty() {
                operaciones_texto.push(Operation::new("BT", vec![]));
                operaciones_texto.push(Operation::new("Tr", vec![3.into()]));

                for bloque in &bloques_con_texto {
                    let bbox = &bloque.bounding_box;

                    let alto_bloque_pt = Self::px_a_pt(bbox.height);
                    let tamano_fuente = (alto_bloque_pt * FACTOR_TAMANO_FUENTE)
                        .max(TAMANO_FUENTE_MINIMO_PT)
                        .min(TAMANO_FUENTE_MAXIMO_PT);

                    let x_pt = Self::px_a_pt(bbox.x);
                    let y_pt = alto_pt - Self::px_a_pt(bbox.y) - alto_bloque_pt;

                    operaciones_texto.push(Operation::new(
                        "Tf",
                        vec!["F1".into(), tamano_fuente.into()],
                    ));
                    operaciones_texto.push(Operation::new("Td", vec![x_pt.into(), y_pt.into()]));
                    operaciones_texto.push(Operation::new(
                        "Tj",
                        vec![Object::string_literal(bloque.content.clone())],
                    ));
                }

                operaciones_texto.push(Operation::new("ET", vec![]));
            }

            let mut todas_las_ops = operaciones_imagen;
            todas_las_ops.extend(operaciones_texto);

            let content = Content {
                operations: todas_las_ops,
            };
            let content_id = doc.add_object(lopdf::Stream::new(
                dictionary! {},
                content.encode().map_err(|e| {
                    ExportError::SerializationError(format!(
                        "Error codificando content stream: {}",
                        e
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
            "Count" => job.document.pages.len() as u32,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        doc.compress();
        doc.save(output_path)
            .map_err(|e| ExportError::SerializationError(format!("Error guardando PDF: {}", e)))?;

        log::info!(
            "PDF Sandwich exportado: {} paginas -> {:?}",
            job.document.pages.len(),
            output_path
        );
        Ok(())
    }

    fn format_name(&self) -> &str {
        "PDF Sandwich"
    }
}

impl PdfSandwichExporter {
    /// Decodifica los bytes de imagen y crea un XObject Image en el documento PDF.
    ///
    /// Retorna el ObjectId del XObject y el nombre de recurso asignado.
    fn crear_xobject_imagen(
        doc: &mut lopdf::Document,
        datos_imagen: &[u8],
    ) -> Result<(lopdf::ObjectId, String), String> {
        let imagen_dyn =
            image::load_from_memory(datos_imagen).map_err(|e| format!("Decodificacion: {}", e))?;

        let rgb = imagen_dyn.to_rgb8();
        let (ancho, alto) = rgb.dimensions();
        let pixeles_raw = rgb.into_raw();

        let img_dict = lopdf::dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => ancho as i64,
            "Height" => alto as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        };

        let img_stream = lopdf::Stream::new(img_dict, pixeles_raw);
        let xobj_id = doc.add_object(img_stream);

        let nombre = format!("Im{}", xobj_id.0);

        Ok((xobj_id, nombre))
    }
}

/// Exportador a JSON estructurado para integración y depuración.
///
/// A diferencia de TXT o PDF, este formato prioriza preservación completa
/// de metadatos y estructura para integraciones posteriores o pruebas.
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
        log::info!("Exportando trabajo {} a JSON: {:?}", job.id, output_path);

        let json_content = serde_json::to_string_pretty(job)
            .map_err(|e| ExportError::SerializationError(e.to_string()))?;

        fs::write(output_path, json_content)?;
        log::info!("Exportacion JSON completada");
        Ok(())
    }

    fn format_name(&self) -> &str {
        "JSON"
    }
}
