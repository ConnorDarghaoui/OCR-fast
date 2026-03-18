use crate::domain::{AlignmentHint, EmphasisHint, TableCellAlignment, TableStructure};

/// Plan exportable de LaTeX más assets externos requeridos.
pub struct LatexExportPlan {
    /// Documento tipado listo para serializar a `.tex`.
    pub document: LatexDocument,
    /// Assets raster que deben persistirse junto al documento.
    pub assets: Vec<LatexAsset>,
}

/// Documento LaTeX tipado con preámbulo y nodos de cuerpo.
pub struct LatexDocument {
    /// Clase base del documento.
    pub document_class: String,
    /// Paquetes requeridos por el renderer.
    pub packages: Vec<LatexPackage>,
    /// Comandos del preámbulo previos a `\\begin{document}`.
    pub preamble: Vec<String>,
    /// Nodos ordenados del cuerpo.
    pub body: Vec<LatexNode>,
}

/// Declaración de paquete de LaTeX.
pub struct LatexPackage {
    /// Nombre canónico del paquete.
    pub name: String,
    /// Opciones serializadas en orden estable.
    pub options: Vec<String>,
}

/// Nodo de cuerpo del documento LaTeX.
pub enum LatexNode {
    /// Bloque posicionado absoluto sobre la página.
    PositionedBlock(LatexTextBlock),
    /// Salto explícito de página.
    PageBreak,
}

/// Bloque `textblock*` con geometría fija.
pub struct LatexTextBlock {
    /// Ancho disponible del bloque.
    pub width_pt: f64,
    /// Coordenada X superior izquierda en puntos.
    pub x_pt: f64,
    /// Coordenada Y superior izquierda en puntos.
    pub y_pt: f64,
    /// Contenido tipado del bloque.
    pub content: LatexContent,
}

/// Contenido permitido dentro de un bloque posicionado.
pub enum LatexContent {
    /// Párrafo editable con estilo tipográfico.
    Paragraph(LatexParagraph),
    /// Tabla tipada con metadata enriquecida.
    Table(LatexTable),
    /// Imagen externa referenciada por asset.
    Image(LatexImage),
    /// Mensaje de degradación controlada.
    FallbackBox(String),
}

/// Párrafo LaTeX con hints ya resueltos.
pub struct LatexParagraph {
    /// Texto visible del párrafo.
    pub text: String,
    /// Alineación deseada.
    pub alignment: AlignmentHint,
    /// Intensidad visual.
    pub emphasis: EmphasisHint,
    /// Escala relativa respecto al cuerpo base.
    pub font_scale: f32,
}

/// Tabla LaTeX renderizable sin reinterpretar OCR crudo.
pub struct LatexTable {
    /// Estructura tabular enriquecida.
    pub table: TableStructure,
    /// Ancho total disponible para la tabla.
    pub width_pt: f64,
}

/// Referencia a imagen que el renderer debe incluir.
pub struct LatexImage {
    /// Ruta relativa desde el `.tex` generado.
    pub relative_path: String,
    /// Ancho final del recurso.
    pub width_pt: f64,
    /// Alto final del recurso.
    pub height_pt: f64,
}

/// Asset binario requerido por el plan LaTeX.
pub struct LatexAsset {
    /// Nombre de archivo relativo dentro del directorio de assets.
    pub file_name: String,
    /// Bytes del asset.
    pub bytes: Vec<u8>,
}

/// Serializa un `LatexDocument` a fuente `.tex`.
pub fn render_document(document: &LatexDocument) -> String {
    let mut contenido = String::new();
    contenido.push_str(&format!("\\documentclass{{{}}}\n", document.document_class));

    for package in &document.packages {
        if package.options.is_empty() {
            contenido.push_str(&format!("\\usepackage{{{}}}\n", package.name));
        } else {
            contenido.push_str(&format!(
                "\\usepackage[{}]{{{}}}\n",
                package.options.join(","),
                package.name
            ));
        }
    }

    for comando in &document.preamble {
        contenido.push_str(comando);
        if !comando.ends_with('\n') {
            contenido.push('\n');
        }
    }

    contenido.push_str("\\begin{document}\n");
    for nodo in &document.body {
        contenido.push_str(&render_node(nodo));
    }
    contenido.push_str("\\end{document}\n");
    contenido
}

fn render_node(node: &LatexNode) -> String {
    match node {
        LatexNode::PositionedBlock(block) => render_text_block(block),
        LatexNode::PageBreak => "\\newpage\n".to_string(),
    }
}

fn render_text_block(block: &LatexTextBlock) -> String {
    let mut contenido = String::new();
    contenido.push_str(&format!(
        "\\begin{{textblock*}}{{{:.2}pt}}({:.2}pt,{:.2}pt)\n",
        block.width_pt, block.x_pt, block.y_pt
    ));
    contenido.push_str(&render_content(&block.content));
    contenido.push_str("\\end{textblock*}\n");
    contenido
}

fn render_content(content: &LatexContent) -> String {
    match content {
        LatexContent::Paragraph(paragraph) => render_paragraph(paragraph),
        LatexContent::Table(table) => render_table(table),
        LatexContent::Image(image) => format!(
            "\\includegraphics[width={:.2}pt,height={:.2}pt]{{{}}}\n",
            image.width_pt,
            image.height_pt,
            escape_latex(&image.relative_path)
        ),
        LatexContent::FallbackBox(message) => {
            format!("\\fbox{{{}}}\n", escape_latex(message))
        }
    }
}

fn render_paragraph(paragraph: &LatexParagraph) -> String {
    let mut contenido = String::new();
    let tamano = (11.0 * paragraph.font_scale as f64).clamp(9.0, 24.0);
    let interlineado = (tamano * 1.18).clamp(10.0, 28.0);
    contenido.push_str(&format!(
        "\\fontsize{{{tamano:.2}pt}}{{{interlineado:.2}pt}}\\selectfont\n"
    ));
    contenido.push_str(match paragraph.alignment {
        AlignmentHint::Center => "\\centering\n",
        AlignmentHint::Right => "\\raggedleft\n",
        AlignmentHint::Left | AlignmentHint::FullWidth => "\\RaggedRight\n",
    });

    let texto_escapado = escape_latex(&paragraph.text);
    if paragraph.emphasis == EmphasisHint::Strong {
        contenido.push_str(&format!("\\textbf{{{texto_escapado}}}\n"));
    } else {
        contenido.push_str(&texto_escapado);
        contenido.push('\n');
    }
    contenido
}

fn render_table(table: &LatexTable) -> String {
    if table.table.rows.is_empty() || table.table.num_cols == 0 {
        return "[Tabla vacia]\n".to_string();
    }

    let columnas = table.table.num_cols.max(1) as usize;
    let anchos_columna = latex_column_widths(&table.table, table.width_pt, columnas);
    let especificacion = anchos_columna
        .iter()
        .map(|ancho| format!("|p{{{ancho:.2}pt}}"))
        .collect::<String>()
        + "|";

    let mut contenido = String::new();
    contenido.push_str("\\renewcommand{\\arraystretch}{1.05}\n");
    contenido.push_str(&format!(
        "\\begin{{tabular}}{{{especificacion}}}\n\\hline\n"
    ));

    for (fila_index, fila) in table.table.rows.iter().enumerate() {
        let celdas = fila
            .iter()
            .map(|celda| {
                let texto = escape_latex(&celda.content);
                let texto = if table.table.is_header_row(fila_index)
                    || celda
                        .style
                        .as_ref()
                        .is_some_and(|style| style.is_emphasized)
                {
                    format!("\\textbf{{{texto}}}")
                } else {
                    texto
                };

                match celda
                    .style
                    .as_ref()
                    .map(|style| style.alignment)
                    .unwrap_or_else(|| {
                        if table.table.is_header_row(fila_index) {
                            TableCellAlignment::Center
                        } else {
                            TableCellAlignment::Left
                        }
                    }) {
                    TableCellAlignment::Left => format!("\\raggedright\\arraybackslash {texto}"),
                    TableCellAlignment::Center => format!("\\centering\\arraybackslash {texto}"),
                    TableCellAlignment::Right => format!("\\raggedleft\\arraybackslash {texto}"),
                }
            })
            .collect::<Vec<_>>();
        contenido.push_str(&celdas.join(" & "));
        contenido.push_str(" \\\\\n\\hline\n");
    }

    contenido.push_str("\\end{tabular}\n");
    contenido
}

fn latex_column_widths(tabla: &TableStructure, ancho_total_pt: f64, columnas: usize) -> Vec<f64> {
    if tabla.column_widths.len() != columnas || tabla.column_widths.iter().all(|ancho| *ancho == 0)
    {
        let ancho_columna = (ancho_total_pt / columnas as f64).max(48.0);
        return vec![ancho_columna; columnas];
    }

    let suma = tabla.column_widths.iter().sum::<u32>().max(1) as f64;
    tabla
        .column_widths
        .iter()
        .map(|ancho| ((ancho_total_pt * (*ancho as f64 / suma)).max(48.0)).min(ancho_total_pt))
        .collect()
}

pub(crate) fn escape_latex(texto: &str) -> String {
    texto
        .chars()
        .flat_map(|c| match c {
            '\\' => "\\textbackslash{}".chars().collect::<Vec<_>>(),
            '{' => "\\{".chars().collect(),
            '}' => "\\}".chars().collect(),
            '$' => "\\$".chars().collect(),
            '&' => "\\&".chars().collect(),
            '%' => "\\%".chars().collect(),
            '#' => "\\#".chars().collect(),
            '_' => "\\_".chars().collect(),
            '^' => "\\^{}".chars().collect(),
            '~' => "\\~{}".chars().collect(),
            _ => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_document_serializa_ast_latex() {
        let document = LatexDocument {
            document_class: "article".to_string(),
            packages: vec![LatexPackage {
                name: "graphicx".to_string(),
                options: Vec::new(),
            }],
            preamble: vec!["\\pagestyle{empty}".to_string()],
            body: vec![
                LatexNode::PositionedBlock(LatexTextBlock {
                    width_pt: 120.0,
                    x_pt: 24.0,
                    y_pt: 42.0,
                    content: LatexContent::Paragraph(LatexParagraph {
                        text: "Hola & mundo".to_string(),
                        alignment: AlignmentHint::Left,
                        emphasis: EmphasisHint::Strong,
                        font_scale: 1.0,
                    }),
                }),
                LatexNode::PageBreak,
            ],
        };

        let rendered = render_document(&document);
        assert!(rendered.contains("\\documentclass{article}"));
        assert!(rendered.contains("\\usepackage{graphicx}"));
        assert!(rendered.contains("\\begin{textblock*}{120.00pt}(24.00pt,42.00pt)"));
        assert!(rendered.contains("\\textbf{Hola \\& mundo}"));
        assert!(rendered.contains("\\newpage"));
    }
}
