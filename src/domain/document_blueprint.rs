use crate::domain::{Dimensions, Rectangle, TableStructure};

/// Representación intermedia para exportadores de alta fidelidad.
///
/// El blueprint desacopla OCR/layout de los detalles de `LaTeX` o PDF
/// reconstruido. Su objetivo es preservar suficiente geometría y semántica para
/// reconstruir el documento sin obligar a cada exportador a reinterpretar el
/// árbol crudo de `Document`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentBlueprint {
    /// Identificador lógico del documento de origen.
    pub document_id: String,
    /// Ruta original serializada para trazabilidad de exportación.
    pub source_path: String,
    /// Estrategia de reconstrucción elegida para este documento.
    pub processing_mode: ProcessingMode,
    /// Páginas reconstruidas del documento.
    pub pages: Vec<PageBlueprint>,
}

/// Estrategia de reconstrucción visual aplicada al documento.
///
/// `DocumentReconstruction` asume un documento clásico y habilita heurísticas
/// de orden de lectura, columnas y hints semánticos. `VisualPreservation`
/// desactiva reinterpretaciones agresivas y prioriza mantener la página
/// original como verdad visual.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ProcessingMode {
    /// Reconstrucción documental con reordenamiento y enriquecimiento semántico.
    DocumentReconstruction,
    /// Preservación visual de la página con OCR auxiliar para búsqueda.
    VisualPreservation,
}

/// Vista de una página lista para exportación visual.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageBlueprint {
    /// Índice de página 1-based.
    pub number: u32,
    /// Dimensiones raster originales.
    pub dimensions: Dimensions,
    /// Elementos visuales en orden canónico.
    pub elements: Vec<ElementBlueprint>,
}

/// Elemento exportable derivado de un bloque OCR.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElementBlueprint {
    /// Rol visual/semántico que guía el exportador final.
    pub role: ElementRole,
    /// Caja original en coordenadas de página.
    pub bounding_box: Rectangle,
    /// Orden de lectura canónico.
    pub reading_order: u32,
    /// Índice de columna sugerido para maquetación.
    pub column_index: u32,
    /// Número total de columnas estimado para la banda visual.
    pub total_columns: u32,
    /// Contenido textual cuando aplica.
    pub text: String,
    /// Confianza del OCR asociada al bloque, si existe una lectura textual.
    pub ocr_confidence: Option<f32>,
    /// Confianza del detector de layout, si la etapa estuvo presente.
    pub layout_confidence: Option<f32>,
    /// Marca conservadora para posibles encabezados repetidos entre páginas.
    pub suspected_header: bool,
    /// Marca conservadora para posibles pies repetidos entre páginas.
    pub suspected_footer: bool,
    /// Estructura tabular cuando el elemento representa una tabla.
    pub table: Option<TableStructure>,
    /// Referencia de recorte para imágenes preservadas del original.
    pub image_crop: Option<ImageCropRef>,
    /// Pistas de estilo útiles para exportadores ricos.
    pub style: StyleHints,
}

/// Rol exportable del elemento dentro del documento final.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ElementRole {
    Title,
    Paragraph,
    Table,
    Figure,
    Formula,
    ListItem,
    Signature,
    Stamp,
    Separator,
    Unknown,
}

/// Referencia ligera a una imagen derivable del documento original.
///
/// El blueprint no duplica bytes raster para evitar presión innecesaria de RAM.
/// Los exportadores que necesiten la imagen podrán recortar bajo demanda desde
/// la página fuente mientras `image_data` siga disponible en memoria.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImageCropRef {
    /// Página de origen desde la cual se debe recortar la imagen.
    pub page_number: u32,
    /// Bounding box del recorte dentro de la página.
    pub bounding_box: Rectangle,
}

/// Pistas tipográficas y de maquetación inferidas del layout.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StyleHints {
    /// Alineación sugerida para el bloque.
    pub alignment: AlignmentHint,
    /// Intensidad semántica sugerida.
    pub emphasis: EmphasisHint,
    /// Escala relativa de fuente respecto al cuerpo base.
    pub font_scale: f32,
    /// Espaciado vertical previo sugerido para reflujo editable.
    pub spacing_before_pt: f32,
    /// Sangría izquierda sugerida dentro de la banda o columna activa.
    pub left_indent_pt: f32,
    /// Indica si conviene mantener el siguiente bloque junto al actual.
    pub keep_with_next: bool,
    /// Indica si el exportador debe preservar posición más que reflujo textual.
    pub preserve_positioning: bool,
}

/// Alineación sugerida para exportadores ricos.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum AlignmentHint {
    Left,
    Center,
    Right,
    FullWidth,
}

/// Intensidad visual sugerida para el contenido.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum EmphasisHint {
    Regular,
    Strong,
    Neutral,
}
