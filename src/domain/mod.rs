//! Modulo de dominio: Representa las entidades y reglas de negocio fundamentales del sistema OCR.
//!
//! Este modulo contiene definiciones de alto nivel que no dependen de tecnologias
//! externas ni frameworks. Las entidades aqui definidas son el nucleo del sistema
//! y representan conceptos puros del dominio OCR.

pub mod errors;

use std::collections::HashMap;
use std::path::PathBuf;

/// Serializacion/deserializacion de `SystemTime` como segundos Unix (u64).
mod serde_system_time {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(time: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let secs = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();
        s.serialize_u64(secs)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

/// Serializacion/deserializacion de `Option<SystemTime>` como segundos Unix opcionales.
mod serde_option_system_time {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(time: &Option<SystemTime>, s: S) -> Result<S::Ok, S::Error> {
        match time {
            Some(t) => {
                let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();
                s.serialize_some(&secs)
            }
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SystemTime>, D::Error> {
        let secs: Option<u64> = Option::deserialize(d)?;
        Ok(secs.map(|s| UNIX_EPOCH + Duration::from_secs(s)))
    }
}

/// Representa un documento digital que puede contener multiples paginas.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Document {
    /// Identificador unico del documento.
    pub id: String,
    /// Ruta original del archivo fuente.
    pub source_path: PathBuf,
    /// Paginas contenidas en el documento.
    pub pages: Vec<Page>,
    /// Metadatos adicionales (formato, autor, etc.).
    pub metadata: HashMap<String, String>,
}

/// Representa una pagina individual dentro de un documento.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Page {
    /// Numero de pagina (comenzando desde 1).
    pub number: u32,
    /// Dimensiones fisicas de la pagina (en pixeles).
    pub dimensions: Dimensions,
    /// Bloques de contenido detectados en la pagina.
    pub blocks: Vec<Block>,
    /// Datos de imagen de la pagina (bytes raw PNG).
    /// No se persisten: son datos derivados que se regeneran al reprocesar.
    #[serde(skip)]
    pub image_data: Option<Vec<u8>>,
}

/// Dimensiones de una pagina o bloque en pixeles.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Dimensions {
    /// Ancho en pixeles.
    pub width: u32,
    /// Alto en pixeles.
    pub height: u32,
}

/// Representa un bloque de contenido en una pagina (texto, imagen, tabla, etc.).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// Tipo de bloque detectado.
    pub block_type: BlockType,
    /// Rectangulo que encierra el bloque (coordenadas absolutas).
    pub bounding_box: Rectangle,
    /// Contenido textual extraido (vacio si no aplica o aun no procesado).
    pub content: String,
    /// Nivel de confianza de la deteccion (0.0 a 1.0).
    pub confidence: f64,
    /// Imagen recortada embebida (para figuras, sellos, firmas).
    /// No se persiste: dato derivado de gran tamano.
    #[serde(skip)]
    pub embedded_image: Option<Vec<u8>>,
    /// Estructura de tabla parseada (solo para BlockType::Table).
    pub table_structure: Option<TableStructure>,
    /// Orden de lectura dentro de la pagina (0-indexed).
    pub reading_order: u32,
}

/// Tipos posibles de bloques de contenido.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum BlockType {
    /// Bloque de texto regular.
    Text,
    /// Titulo o encabezado.
    Title,
    /// Tabla de datos.
    Table,
    /// Imagen o figura.
    Image,
    /// Formula matematica.
    Formula,
    /// Lista (enumerada o con vinetas).
    List,
    /// Firma manuscrita.
    Signature,
    /// Sello oficial.
    Stamp,
    /// Separador o linea divisoria.
    Separator,
    /// Tipo no identificado.
    Unknown,
}

/// Estructura de una tabla detectada por el modelo Table Transformer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableStructure {
    /// Filas de la tabla, cada fila contiene celdas.
    pub rows: Vec<Vec<TableCell>>,
    /// Numero total de filas.
    pub num_rows: u32,
    /// Numero total de columnas.
    pub num_cols: u32,
}

impl TableStructure {
    /// Convierte la tabla a formato Markdown.
    pub fn to_markdown(&self) -> String {
        if self.rows.is_empty() {
            return String::new();
        }

        let mut resultado = String::new();

        for (i, fila) in self.rows.iter().enumerate() {
            let celdas: Vec<String> = fila.iter()
                .map(|c| c.content.clone())
                .collect();
            resultado.push_str(&format!("| {} |\n", celdas.join(" | ")));

            // Separador despues del header
            if i == 0 {
                let separadores: Vec<String> = fila.iter()
                    .map(|_| "---".to_string())
                    .collect();
                resultado.push_str(&format!("| {} |\n", separadores.join(" | ")));
            }
        }

        resultado
    }
}

/// Celda individual dentro de una tabla.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableCell {
    /// Contenido textual de la celda.
    pub content: String,
    /// Cuantas filas abarca esta celda.
    pub row_span: u32,
    /// Cuantas columnas abarca esta celda.
    pub col_span: u32,
    /// Rectangulo de la celda en coordenadas de la tabla.
    pub bounding_box: Rectangle,
}

/// Rectangulo con coordenadas absolutas en pixeles.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Rectangle {
    /// Coordenada X de la esquina superior izquierda.
    pub x: u32,
    /// Coordenada Y de la esquina superior izquierda.
    pub y: u32,
    /// Ancho del rectangulo.
    pub width: u32,
    /// Alto del rectangulo.
    pub height: u32,
}

/// Formato de exportacion deseado para el documento procesado.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Markdown con estructura de bloques.
    #[default]
    Markdown,
    /// PDF tipo sandwich (imagen + texto invisible seleccionable).
    Pdf,
    /// JSON con el Job completo serializado.
    Json,
}

impl OutputFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Pdf      => "pdf",
            Self::Json     => "json",
        }
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Pdf      => "PDF",
            Self::Json     => "JSON",
        }
    }

    pub const OPCIONES: &'static [OutputFormat] = &[
        OutputFormat::Markdown,
        OutputFormat::Pdf,
        OutputFormat::Json,
    ];
}

/// Estado de un trabajo de procesamiento OCR.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    /// Trabajo en cola esperando procesamiento.
    Queued,
    /// Trabajo actualmente en procesamiento.
    Processing,
    /// Trabajo completado exitosamente.
    Completed,
    /// Trabajo fallido con error.
    Failed,
    /// Trabajo cancelado por el usuario.
    Cancelled,
}

/// Representa una tarea de procesamiento OCR.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Job {
    /// Identificador unico del trabajo.
    pub id: String,
    /// Documento asociado al trabajo.
    pub document: Document,
    /// Estado actual del trabajo.
    pub status: JobStatus,
    /// Marca de tiempo de creacion (segundos Unix).
    #[serde(with = "serde_system_time")]
    pub created_at: std::time::SystemTime,
    /// Marca de tiempo de finalizacion (segundos Unix, si aplica).
    #[serde(with = "serde_option_system_time")]
    pub completed_at: Option<std::time::SystemTime>,
    /// Perfil de procesamiento utilizado.
    pub profile: ProcessingProfile,
    /// Mensaje de error (solo si status == Failed).
    pub error_message: Option<String>,
    /// Formato de salida solicitado por el usuario.
    #[serde(default)]
    pub formato_salida: OutputFormat,
}

/// Perfil de procesamiento que determina la estrategia OCR utilizada.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ProcessingProfile {
    /// Prioriza velocidad sobre precision.
    Fast,
    /// Prioriza precision sobre velocidad.
    Accurate,
    /// Compromiso equilibrado entre velocidad y precision.
    Balanced,
}

impl Default for ProcessingProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Configuracion de idioma para OCR.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LanguageConfig {
    /// Codigo ISO 639-3 del idioma principal (ej: "spa", "eng").
    pub primary: String,
    /// Idiomas secundarios para documentos multilingues.
    pub secondary: Vec<String>,
}

impl Default for LanguageConfig {
    fn default() -> Self {
        Self {
            primary: "spa".to_string(),
            secondary: vec![],
        }
    }
}
