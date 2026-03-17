/// Tipos de error de dominio y fronteras de fallo propagables entre capas.
pub mod errors;

use std::collections::HashMap;
use std::path::PathBuf;

/// Serializacion/deserializacion de `SystemTime` como segundos Unix (u64).
mod serde_system_time {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub(super) fn serialize<S: Serializer>(time: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let secs = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        s.serialize_u64(secs)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

/// Serializacion/deserializacion de `Option<SystemTime>` como segundos Unix opcionales.
mod serde_option_system_time {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub(super) fn serialize<S: Serializer>(
        time: &Option<SystemTime>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match time {
            Some(t) => {
                let secs = t
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs();
                s.serialize_some(&secs)
            }
            None => s.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<SystemTime>, D::Error> {
        let secs: Option<u64> = Option::deserialize(d)?;
        Ok(secs.map(|s| UNIX_EPOCH + Duration::from_secs(s)))
    }
}

/// Agregado raíz del dominio para una unidad de ingesta OCR.
///
/// `Document` encapsula el archivo de origen, las páginas derivadas y la metadata
/// suficiente para que el pipeline sea reejecutable y serializable sin depender de
/// detalles de UI, storage o engines concretos. El modelo separa explícitamente
/// identidad (`id`) de ubicación física (`source_path`) para permitir persistencia,
/// reintentos y exportación aun cuando la capa de infraestructura cambie.
///
/// El diseño evita referencias prestadas y usa ownership completo para reducir
/// fricción con el borrow checker en etapas mutables del pipeline. Eso incrementa
/// el costo de clonación frente a un modelo altamente prestado, pero simplifica
/// concurrencia, persistencia y paso por canales entre hilos.
///
/// # Performance
///
/// La estructura prioriza mutación local por página y serialización simple.
/// `Vec<Page>` y `HashMap<String, String>` ofrecen costos predecibles y evitan
/// layouts de memoria acoplados a un formato OCR específico.
///
/// # Trade-offs
///
/// No modela streaming ni paginación perezosa. Esa decisión favorece ergonomía
/// de aplicación y consistencia del estado a costa de mayor presión de memoria
/// en documentos extremadamente grandes.
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

/// Representa una página materializada dentro del documento de trabajo.
///
/// La página combina geometría, bloques semánticos e imagen derivada. El campo
/// `image_data` queda fuera de la serialización porque es un artefacto pesado y
/// regenerable; persistirlo duplicaría datos que ya existen en el archivo fuente
/// y volvería más costoso el recovery del `JobStore`.
///
/// # Trade-offs
///
/// El modelo usa bytes en memoria en lugar de un handle a archivo para desacoplar
/// fases posteriores de la ubicación del input y evitar dependencias en lifetimes
/// sobre buffers externos.
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

/// Dimensiones discretas en píxeles para páginas y regiones detectadas.
///
/// Se mantiene como tipo nominal separado en lugar de usar tuplas para evitar
/// confusiones entre ancho/alto y para hacer visibles las invariantes geométricas
/// en firmas públicas.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Dimensions {
    /// Ancho en pixeles.
    pub width: u32,
    /// Alto en pixeles.
    pub height: u32,
}

/// Unidad semántica mínima procesable dentro de una página.
///
/// `Block` es el contrato intermedio compartido entre layout, OCR, tablas,
/// exportadores y UI. Concentrar aquí la información evita N estructuras de
/// traducción entre fases y reduce errores de sincronización entre coordenadas,
/// contenido y confianza.
///
/// `embedded_image` se marca como dato derivado y no persistible porque aumenta
/// el tamaño de snapshot de forma desproporcionada y no aporta valor para la
/// mayoría de flujos de recuperación o reexportación textual.
///
/// # Performance
///
/// Mantener el bounding box y el orden de lectura junto al contenido permite que
/// exportadores y postprocesadores operen en un solo recorrido lineal por bloque.
///
/// # Trade-offs
///
/// El tipo mezcla metadata estructural y payload OCR. Es menos puro que separar
/// DTOs por fase, pero evita reconstrucciones costosas y mantiene el pipeline
/// observable desde la UI sin adapters adicionales.
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

/// Taxonomía semántica producida por layout y consumida por OCR/exportación.
///
/// El enum es deliberadamente pequeño y estable. Un vocabulario acotado facilita
/// heurísticas, serialización compatible y decisiones de renderizado sin obligar a
/// la UI ni a los exportadores a conocer detalles del modelo detector subyacente.
///
/// # Trade-offs
///
/// Algunas clases quedan necesariamente agregadas bajo categorías amplias. Eso
/// reduce granularidad analítica, pero evita sobreajustar el dominio a un modelo
/// específico de layout que podría cambiar con el tiempo.
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

/// Estructura tabular normalizada a partir de detección y OCR por celda.
///
/// La representación usa filas de `TableCell` en lugar de una malla densa porque
/// los `row_span` y `col_span` son parte del dominio, no un detalle de render.
/// Esto permite reconstruir Markdown, JSON o formatos futuros sin perder la
/// estructura fusionada que observó el detector.
///
/// # Trade-offs
///
/// No impone validación fuerte de rectangularidad. La consistencia final depende
/// del analizador de tablas para poder tolerar resultados parciales o ruidosos
/// sin bloquear el resto del pipeline.
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
    /// Convierte la tabla a Markdown consumible por exportadores y previews.
    ///
    /// La primera fila se trata como encabezado porque es la convención más útil
    /// para lectura humana y porque Markdown no expresa metadata tabular más rica.
    /// El método degrada con gracia ante tablas incompletas en lugar de fallar,
    /// priorizando exportación utilizable sobre exactitud estructural perfecta.
    ///
    /// # Performance
    ///
    /// Recorre la estructura una sola vez y evita buffers intermedios por celda
    /// distintos al `Vec<String>` requerido para `join`.
    ///
    /// # Trade-offs
    ///
    /// Markdown pierde spans y geometría fina. Para consumidores que necesiten
    /// fidelidad estructural total, debe preferirse la representación JSON.
    pub fn to_markdown(&self) -> String {
        if self.rows.is_empty() {
            return String::new();
        }

        let mut resultado = String::new();

        for (i, fila) in self.rows.iter().enumerate() {
            let celdas: Vec<String> = fila.iter().map(|c| c.content.clone()).collect();
            resultado.push_str(&format!("| {} |\n", celdas.join(" | ")));

            // Separador despues del header
            if i == 0 {
                let separadores: Vec<String> = fila.iter().map(|_| "---".to_string()).collect();
                resultado.push_str(&format!("| {} |\n", separadores.join(" | ")));
            }
        }

        resultado
    }
}

/// Celda lógica de una tabla con información de contenido y spans.
///
/// El bounding box permanece en coordenadas de tabla para que exportadores y
/// diagnósticos puedan razonar sobre alineación sin depender del sistema global
/// de coordenadas de la página.
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

/// Bounding box axis-aligned expresado en píxeles absolutos.
///
/// Se usa `u32` porque el dominio trabaja en coordenadas raster no negativas.
/// Evitar signed integers elimina una clase de errores geométricos y simplifica
/// validación de recortes antes de tocar buffers de imagen.
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

/// Formato de materialización final para un `Job` completado.
///
/// Este enum existe en dominio porque la decisión de salida afecta pipeline,
/// persistencia y UI. Mantenerlo fuera de infraestructura evita acoplar flujos
/// de aplicación a nombres concretos de exportadores.
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
    /// Retorna la extensión canónica de archivo para el formato de salida.
    ///
    /// # Notes
    ///
    /// Se usa para generación de rutas y validación de exportadores. Mantener la
    /// tabla aquí centraliza compatibilidad y evita divergencia entre UI y storage.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Pdf => "pdf",
            Self::Json => "json",
        }
    }

    /// Retorna el nombre estable mostrado en UI y logs operativos.
    ///
    /// # Notes
    ///
    /// Separar nombre visible de la extensión permite evolucionar presentación sin
    /// tocar flujos de persistencia ni convenciones de archivos.
    pub fn nombre(&self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Pdf => "PDF",
            Self::Json => "JSON",
        }
    }

    /// Lista ordenada de formatos ofrecidos por la interfaz actual.
    ///
    /// # Trade-offs
    ///
    /// Se expone como slice estático para evitar asignaciones y para preservar un
    /// orden determinista en menús y pruebas de snapshot.
    pub const OPCIONES: &'static [OutputFormat] = &[
        OutputFormat::Markdown,
        OutputFormat::Pdf,
        OutputFormat::Json,
    ];
}

/// Máquina de estados observable de un trabajo OCR.
///
/// El enum separa explícitamente `Failed` de `Cancelled` porque ambas rutas tienen
/// implicaciones operativas distintas: la primera sugiere investigar o reintentar,
/// la segunda preserva la intención del usuario y no debe contaminar métricas de
/// error de plataforma.
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

/// Agregado operativo que rastrea una ejecución OCR end-to-end.
///
/// `Job` encapsula el documento, su estado y la metadata temporal necesaria para
/// observabilidad, persistencia y exportación. El tipo está diseñado para viajar
/// entre almacenamiento, UI y pipeline sin proyecciones intermedias.
///
/// # Performance
///
/// El snapshot completo simplifica persistencia y recuperación a costa de tamaños
/// de escritura mayores que un modelo totalmente normalizado.
///
/// # Trade-offs
///
/// No modela eventos incrementales. La decisión prioriza reanudación simple y
/// lectura directa desde la UI por encima de un event sourcing más complejo.
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

/// Política de calidad/latencia aplicada al pipeline OCR.
///
/// El perfil modifica umbrales y profundidad del procesamiento sin cambiar la
/// semántica del resultado final. Esto permite exponer una decisión de producto
/// estable mientras la implementación interna evoluciona.
///
/// # Trade-offs
///
/// Un enum discreto es menos flexible que parámetros numéricos abiertos, pero
/// evita combinaciones inválidas y mantiene el comportamiento testeable.
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

/// Preferencias de idioma usadas para selección y ajuste de OCR.
///
/// El dominio separa idioma primario de secundarios porque muchos motores OCR
/// optimizan mejor cuando reciben una lengua dominante y un conjunto reducido de
/// candidatos auxiliares. El modelo mantiene esa intención sin quedar atado a la
/// sintaxis concreta de ningún backend.
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
