use super::common::{
    asegurar_directorio_padre, construir_blueprint, directorio_assets, obtener_pagina, px_a_pt,
    recortar_imagen_desde_referencia,
};
use super::latex_ast::{
    render_document, LatexAsset, LatexContent, LatexDocument, LatexExportPlan, LatexImage,
    LatexNode, LatexPackage, LatexParagraph, LatexTable, LatexTextBlock,
};
use crate::domain::errors::ExportError;
use crate::domain::{
    ElementBlueprint, ElementRole, Job, ProcessingMode, Rectangle,
};
use crate::interfaces::ports::ExporterPort;
use std::fs;
use std::path::Path;

/// Umbral conservador para preservar recortes raster en LaTeX facsímil.
const LATEX_FACSIMILE_OCR_CONFIDENCE_THRESHOLD: f32 = 0.74;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatexExportMode {
    Semantic,
    Facsimile,
}

/// Exportador LaTeX configurable entre reflujo semántico y facsímil.
pub struct LatexExporter {
    mode: LatexExportMode,
}

impl LatexExporter {
    /// Construye el exportador LaTeX semántico por defecto.
    pub fn new() -> Self {
        Self::new_semantic()
    }

    /// Construye el exportador LaTeX orientado a edición y estructura lógica.
    pub fn new_semantic() -> Self {
        Self {
            mode: LatexExportMode::Semantic,
        }
    }

    /// Construye el exportador LaTeX facsímil basado en posicionamiento.
    pub fn new_facsimile() -> Self {
        Self {
            mode: LatexExportMode::Facsimile,
        }
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
        let directorio_assets = directorio_assets(output_path);
        fs::create_dir_all(&directorio_assets)?;

        let nombre_directorio_assets = directorio_assets
            .file_name()
            .and_then(|valor| valor.to_str())
            .unwrap_or("documento_assets");
        let plan = match self.mode {
            _ if blueprint.processing_mode == ProcessingMode::VisualPreservation => {
                construir_plan_latex_visual_preservation(job, &blueprint, nombre_directorio_assets)?
            }
            LatexExportMode::Semantic => {
                construir_plan_latex_semantico(job, &blueprint, nombre_directorio_assets)?
            }
            LatexExportMode::Facsimile => {
                construir_plan_latex_facsimil(job, &blueprint, nombre_directorio_assets)?
            }
        };
        for asset in &plan.assets {
            fs::write(directorio_assets.join(&asset.file_name), &asset.bytes)?;
        }

        fs::write(output_path, render_document(&plan.document))?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "LaTeX"
    }
}

fn construir_plan_latex_semantico(
    job: &Job,
    blueprint: &crate::domain::DocumentBlueprint,
    nombre_directorio_assets: &str,
) -> Result<LatexExportPlan, ExportError> {
    let mut body = Vec::new();
    let mut assets = Vec::new();

    for (indice_pagina, pagina) in blueprint.pages.iter().enumerate() {
        let elementos_visibles: Vec<&ElementBlueprint> = pagina
            .elements
            .iter()
            .filter(|elemento| !elemento.suspected_header && !elemento.suspected_footer)
            .collect();
        let body_len_inicial = body.len();
        let mut indice = 0usize;

        while indice < elementos_visibles.len() {
            let elemento = elementos_visibles[indice];
            match elemento.role {
                ElementRole::Title => {
                    let titulo = elemento.text.trim();
                    if !titulo.is_empty() {
                        body.push(LatexNode::Section {
                            title: titulo.to_string(),
                            numbered: false,
                        });
                    }
                    indice += 1;
                }
                ElementRole::ListItem => {
                    let mut items = Vec::new();
                    while indice < elementos_visibles.len()
                        && elementos_visibles[indice].role == ElementRole::ListItem
                    {
                        let texto = elementos_visibles[indice].text.trim();
                        if !texto.is_empty() {
                            items.push(texto.to_string());
                        }
                        indice += 1;
                    }
                    if !items.is_empty() {
                        body.push(LatexNode::Itemize(items));
                    }
                }
                ElementRole::Table => {
                    if let Some(ref tabla) = elemento.table {
                        body.push(LatexNode::Table(LatexTable {
                            table: tabla.clone(),
                            width_pt: px_a_pt(elemento.bounding_box.width),
                        }));
                    } else if let Some(parrafo) = construir_parrafo_latex(elemento) {
                        body.push(LatexNode::Paragraph(parrafo));
                    }
                    indice += 1;
                }
                ElementRole::Figure
                | ElementRole::Signature
                | ElementRole::Stamp
                | ElementRole::Formula => {
                    if let Some(imagen) = construir_imagen_latex(
                        job,
                        elemento,
                        &format!("page{}_element{}.png", pagina.number, indice + 1),
                        nombre_directorio_assets,
                        &mut assets,
                    )? {
                        body.push(LatexNode::Figure(imagen));
                    } else if let Some(parrafo) = construir_parrafo_latex(elemento) {
                        body.push(LatexNode::Paragraph(parrafo));
                    }
                    indice += 1;
                }
                ElementRole::Separator => {
                    indice += 1;
                }
                _ => {
                    if let Some(parrafo) = construir_parrafo_latex(elemento) {
                        body.push(LatexNode::Paragraph(parrafo));
                    }
                    indice += 1;
                }
            }
        }

        if indice_pagina + 1 < blueprint.pages.len() && body.len() > body_len_inicial {
            body.push(LatexNode::PageBreak);
        }
    }

    Ok(LatexExportPlan {
        document: LatexDocument {
            document_class: "article".to_string(),
            packages: vec![
                LatexPackage {
                    name: "geometry".to_string(),
                    options: vec!["margin=72pt".to_string()],
                },
                LatexPackage {
                    name: "graphicx".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "array".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "longtable".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "ragged2e".to_string(),
                    options: Vec::new(),
                },
            ],
            preamble: vec![
                "\\setlength{\\parindent}{0pt}".to_string(),
                "\\setlength{\\parskip}{0.6em}".to_string(),
                "\\raggedbottom".to_string(),
            ],
            body,
        },
        assets,
    })
}

fn construir_plan_latex_facsimil(
    job: &Job,
    blueprint: &crate::domain::DocumentBlueprint,
    nombre_directorio_assets: &str,
) -> Result<LatexExportPlan, ExportError> {
    let primera_pagina = blueprint
        .pages
        .first()
        .ok_or_else(|| ExportError::SerializationError("Documento sin páginas".to_string()))?;

    let mut body = Vec::new();
    let mut assets = Vec::new();
    for (indice_pagina, pagina) in blueprint.pages.iter().enumerate() {
        for (indice_elemento, elemento) in pagina.elements.iter().enumerate() {
            let nombre_asset = format!("page{}_element{}.png", pagina.number, indice_elemento + 1);
            body.push(construir_nodo_latex_facsimil(
                job,
                pagina.number,
                elemento,
                &nombre_asset,
                nombre_directorio_assets,
                &mut assets,
            )?);
        }
        if indice_pagina + 1 < blueprint.pages.len() {
            body.push(LatexNode::PageBreak);
        }
    }

    Ok(LatexExportPlan {
        document: LatexDocument {
            document_class: "article".to_string(),
            packages: vec![
                LatexPackage {
                    name: "geometry".to_string(),
                    options: vec![format!(
                        "paperwidth={:.2}pt,paperheight={:.2}pt,margin=0pt",
                        px_a_pt(primera_pagina.dimensions.width),
                        px_a_pt(primera_pagina.dimensions.height)
                    )],
                },
                LatexPackage {
                    name: "textpos".to_string(),
                    options: vec!["absolute".to_string(), "overlay".to_string()],
                },
                LatexPackage {
                    name: "graphicx".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "array".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "longtable".to_string(),
                    options: Vec::new(),
                },
                LatexPackage {
                    name: "ragged2e".to_string(),
                    options: Vec::new(),
                },
            ],
            preamble: vec![
                "\\pagestyle{empty}".to_string(),
                "\\setlength{\\TPHorizModule}{1pt}".to_string(),
                "\\setlength{\\TPVertModule}{1pt}".to_string(),
                "\\setlength{\\parindent}{0pt}".to_string(),
            ],
            body,
        },
        assets,
    })
}

fn construir_plan_latex_visual_preservation(
    job: &Job,
    blueprint: &crate::domain::DocumentBlueprint,
    nombre_directorio_assets: &str,
) -> Result<LatexExportPlan, ExportError> {
    let primera_pagina = blueprint
        .pages
        .first()
        .ok_or_else(|| ExportError::SerializationError("Documento sin páginas".to_string()))?;
    let mut body = Vec::new();
    let mut assets = Vec::new();

    for (indice_pagina, pagina) in blueprint.pages.iter().enumerate() {
        if let Some(imagen) = construir_imagen_latex_pagina_completa(
            job,
            pagina.number,
            &format!("page{}_full.png", pagina.number),
            nombre_directorio_assets,
            &mut assets,
        )? {
            body.push(LatexNode::PositionedBlock(LatexTextBlock {
                width_pt: px_a_pt(pagina.dimensions.width),
                x_pt: 0.0,
                y_pt: 0.0,
                content: LatexContent::Image(imagen),
            }));
        }

        if indice_pagina + 1 < blueprint.pages.len() {
            body.push(LatexNode::PageBreak);
        }
    }

    Ok(LatexExportPlan {
        document: LatexDocument {
            document_class: "article".to_string(),
            packages: vec![
                LatexPackage {
                    name: "geometry".to_string(),
                    options: vec![format!(
                        "paperwidth={:.2}pt,paperheight={:.2}pt,margin=0pt",
                        px_a_pt(primera_pagina.dimensions.width),
                        px_a_pt(primera_pagina.dimensions.height)
                    )],
                },
                LatexPackage {
                    name: "textpos".to_string(),
                    options: vec!["absolute".to_string(), "overlay".to_string()],
                },
                LatexPackage {
                    name: "graphicx".to_string(),
                    options: Vec::new(),
                },
            ],
            preamble: vec![
                "\\pagestyle{empty}".to_string(),
                "\\setlength{\\TPHorizModule}{1pt}".to_string(),
                "\\setlength{\\TPVertModule}{1pt}".to_string(),
                "\\setlength{\\parindent}{0pt}".to_string(),
            ],
            body,
        },
        assets,
    })
}

fn construir_nodo_latex_facsimil(
    job: &Job,
    numero_pagina: u32,
    elemento: &ElementBlueprint,
    nombre_asset: &str,
    nombre_directorio_assets: &str,
    assets: &mut Vec<LatexAsset>,
) -> Result<LatexNode, ExportError> {
    let x_pt = px_a_pt(elemento.bounding_box.x);
    let y_pt = px_a_pt(elemento.bounding_box.y);
    let width_pt = px_a_pt(elemento.bounding_box.width);

    let content = match elemento.role {
        ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
            if let Some(imagen) = construir_imagen_latex(
                job,
                elemento,
                nombre_asset,
                nombre_directorio_assets,
                assets,
            )? {
                LatexContent::Image(imagen)
            } else {
                LatexContent::FallbackBox("Imagen no disponible en memoria".to_string())
            }
        }
        ElementRole::Formula if elemento.style.preserve_positioning => {
            if let Some(imagen) = construir_imagen_latex_desde_bbox(
                job,
                numero_pagina,
                &elemento.bounding_box,
                nombre_asset,
                nombre_directorio_assets,
                assets,
            )? {
                LatexContent::Image(imagen)
            } else {
                LatexContent::FallbackBox("Formula no disponible en memoria".to_string())
            }
        }
        _ if debe_preservarse_como_imagen_en_latex_facsimil(elemento) => {
            if let Some(imagen) = construir_imagen_latex_desde_bbox(
                job,
                numero_pagina,
                &elemento.bounding_box,
                nombre_asset,
                nombre_directorio_assets,
                assets,
            )? {
                LatexContent::Image(imagen)
            } else {
                LatexContent::Paragraph(LatexParagraph {
                    text: elemento.text.clone(),
                    alignment: elemento.style.alignment,
                    emphasis: elemento.style.emphasis,
                    font_scale: elemento.style.font_scale,
                })
            }
        }
        ElementRole::Table => {
            if let Some(ref tabla) = elemento.table {
                LatexContent::Table(LatexTable {
                    table: tabla.clone(),
                    width_pt,
                })
            } else {
                LatexContent::Paragraph(LatexParagraph {
                    text: elemento.text.clone(),
                    alignment: elemento.style.alignment,
                    emphasis: elemento.style.emphasis,
                    font_scale: elemento.style.font_scale,
                })
            }
        }
        _ => LatexContent::Paragraph(LatexParagraph {
            text: elemento.text.clone(),
            alignment: elemento.style.alignment,
            emphasis: elemento.style.emphasis,
            font_scale: elemento.style.font_scale,
        }),
    };

    Ok(LatexNode::PositionedBlock(LatexTextBlock {
        width_pt,
        x_pt,
        y_pt,
        content,
    }))
}

fn debe_preservarse_como_imagen_en_latex_facsimil(elemento: &ElementBlueprint) -> bool {
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
        .is_some_and(|valor| valor < LATEX_FACSIMILE_OCR_CONFIDENCE_THRESHOLD)
}

fn construir_parrafo_latex(elemento: &ElementBlueprint) -> Option<LatexParagraph> {
    let texto = elemento.text.trim();
    if texto.is_empty() {
        return None;
    }

    Some(LatexParagraph {
        text: texto.to_string(),
        alignment: elemento.style.alignment,
        emphasis: elemento.style.emphasis,
        font_scale: elemento.style.font_scale,
    })
}

fn construir_imagen_latex(
    job: &Job,
    elemento: &ElementBlueprint,
    nombre_asset: &str,
    nombre_directorio_assets: &str,
    assets: &mut Vec<LatexAsset>,
) -> Result<Option<LatexImage>, ExportError> {
    let Some(ref imagen) = elemento.image_crop else {
        return Ok(None);
    };

    construir_imagen_latex_desde_bbox(
        job,
        imagen.page_number,
        &imagen.bounding_box,
        nombre_asset,
        nombre_directorio_assets,
        assets,
    )
}

fn construir_imagen_latex_desde_bbox(
    job: &Job,
    numero_pagina: u32,
    bounding_box: &Rectangle,
    nombre_asset: &str,
    nombre_directorio_assets: &str,
    assets: &mut Vec<LatexAsset>,
) -> Result<Option<LatexImage>, ExportError> {
    match recortar_imagen_desde_referencia(job, numero_pagina, bounding_box) {
        Ok(bytes) => {
            assets.push(LatexAsset {
                file_name: nombre_asset.to_string(),
                bytes,
            });
            Ok(Some(LatexImage {
                relative_path: format!("{}/{}", nombre_directorio_assets, nombre_asset),
                width_pt: px_a_pt(bounding_box.width),
                height_pt: px_a_pt(bounding_box.height),
            }))
        }
        Err(_) => Ok(None),
    }
}

fn construir_imagen_latex_pagina_completa(
    job: &Job,
    numero_pagina: u32,
    nombre_asset: &str,
    nombre_directorio_assets: &str,
    assets: &mut Vec<LatexAsset>,
) -> Result<Option<LatexImage>, ExportError> {
    let pagina = obtener_pagina(job, numero_pagina)?;
    let Some(bytes) = pagina.image_data.as_ref() else {
        return Ok(None);
    };

    assets.push(LatexAsset {
        file_name: nombre_asset.to_string(),
        bytes: bytes.clone(),
    });
    Ok(Some(LatexImage {
        relative_path: format!("{}/{}", nombre_directorio_assets, nombre_asset),
        width_pt: px_a_pt(pagina.dimensions.width),
        height_pt: px_a_pt(pagina.dimensions.height),
    }))
}
