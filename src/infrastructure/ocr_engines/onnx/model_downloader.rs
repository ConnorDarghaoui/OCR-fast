use crate::domain::errors::ModelDownloadError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CHUNK_SIZE: usize = 65_536; // 64 KB por lectura
const MAX_REINTENTOS: u32 = 3;
const ESPERA_REINTENTO_SECS: u64 = 2;
const CHUNKS_POR_REPORTE: usize = 8; // reportar progreso cada 512 KB

struct ModelDefinition {
    categoria: &'static str,
    nombre_archivo: &'static str,
    url: &'static str,
    /// SHA256 del archivo descargado en hexadecimal sin prefijo.
    ///
    /// `None` = sin verificacion (aceptar cualquier contenido).
    /// `Some(hash)` = se verifica durante la descarga Y al arrancar si el archivo ya existe.
    ///
    /// TODO: Rellenar con `sha256sum` una vez descargados los modelos por primera vez.
    sha256_esperado: Option<&'static str>,
}

const MODELOS_REQUERIDOS: &[ModelDefinition] = &[
    ModelDefinition {
        categoria: "orientation",
        nombre_archivo: "PP-LCNet_x1_0_doc_ori.onnx",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/preprocessing/doc-orientation/PP-LCNet_x1_0_doc_ori.onnx",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "orientation",
        nombre_archivo: "config.json",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/preprocessing/doc-orientation/config.json",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "layout",
        nombre_archivo: "doclayout_yolo_docstructbench_imgsz1024.onnx",
        url: "https://huggingface.co/wybxc/DocLayout-YOLO-DocStructBench-onnx/resolve/main/doclayout_yolo_docstructbench_imgsz1024.onnx",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "ocr",
        nombre_archivo: "det.onnx",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/detection/v5/det.onnx",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "ocr",
        nombre_archivo: "det_config.json",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/detection/v5/config.json",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "ocr",
        nombre_archivo: "rec.onnx",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/languages/latin/rec.onnx",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "ocr",
        nombre_archivo: "dict.txt",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/main/languages/latin/dict.txt",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "table",
        nombre_archivo: "model_uint8.onnx",
        url: "https://huggingface.co/Xenova/table-transformer-structure-recognition/resolve/main/onnx/model_uint8.onnx",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "table",
        nombre_archivo: "config.json",
        url: "https://huggingface.co/Xenova/table-transformer-structure-recognition/resolve/main/config.json",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
    ModelDefinition {
        categoria: "table",
        nombre_archivo: "preprocessor_config.json",
        url: "https://huggingface.co/Xenova/table-transformer-structure-recognition/resolve/main/preprocessor_config.json",
        sha256_esperado: None, // TODO: completar tras primera descarga
    },
];

/// Gestor local de artefactos ONNX con verificación y reintentos.
///
/// El downloader abstrae una preocupación operativa distinta del engine: red,
/// integridad y almacenamiento local. Mantener esa lógica separada permite que la
/// carga de sesiones ONNX suponga un directorio consistente y no deba lidiar con
/// descargas parciales ni recuperación de corrupción.
///
/// # Trade-offs
///
/// El gestor usa descargas blocking y escritura a disco local, lo que simplifica
/// semántica de errores y recovery en una aplicación de escritorio.
pub struct ModelDownloader {
    directorio_base: PathBuf,
    cliente_http: reqwest::blocking::Client,
}

impl ModelDownloader {
    /// Construye un downloader usando la ruta local estándar de OCRFast.
    pub fn new() -> Result<Self, ModelDownloadError> {
        let directorio_base = dirs::data_local_dir()
            .ok_or_else(|| {
                ModelDownloadError::DirectoryError(
                    "No se pudo obtener directorio local de datos".to_string(),
                )
            })?
            .join("ocrfast")
            .join("models");

        Ok(Self {
            directorio_base,
            cliente_http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|e| ModelDownloadError::NetworkError(e.to_string()))?,
        })
    }

    /// Construye un downloader apuntando a un directorio explícito.
    ///
    /// # Notes
    ///
    /// Esta variante existe para tests, empaquetado y escenarios donde el caller
    /// controla la ubicación física de los modelos.
    pub fn with_directory(directorio: PathBuf) -> Result<Self, ModelDownloadError> {
        Ok(Self {
            directorio_base: directorio,
            cliente_http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|e| ModelDownloadError::NetworkError(e.to_string()))?,
        })
    }

    /// Retorna el directorio base en el que se materializan los modelos.
    pub fn directorio_base(&self) -> &Path {
        &self.directorio_base
    }

    /// Retorna el número total de artefactos requeridos por el pipeline actual.
    pub fn total_modelos_requeridos() -> usize {
        MODELOS_REQUERIDOS.len()
    }

    /// Garantiza que el conjunto completo de modelos exista y sea utilizable.
    ///
    /// La función revalida integridad cuando existe checksum conocido y reintenta
    /// descargas transitorias. Solo retorna éxito cuando el árbol de artefactos
    /// queda en estado consistente para inicializar el engine.
    ///
    /// # Errors
    ///
    /// Retorna `ModelDownloadError` si falla red, filesystem o integridad.
    pub fn asegurar_todos_los_modelos(
        &self,
        on_archivo: Option<&dyn Fn(&str, usize, usize)>,
        on_bytes: Option<&dyn Fn(u64, u64)>,
    ) -> Result<PathBuf, ModelDownloadError> {
        let total = MODELOS_REQUERIDOS.len();
        let mut descargados = 0usize;

        for def in MODELOS_REQUERIDOS {
            let destino = self
                .directorio_base
                .join(def.categoria)
                .join(def.nombre_archivo);

            if destino.exists() {
                // Si el modelo ya existe y tiene checksum, reverificar integridad en disco.
                // Esto previene que una descarga parcial previa (sin checksum en esa epoca)
                // pase desapercibida y crashee ONNX Runtime al cargar.
                if let Some(hash_esperado) = def.sha256_esperado {
                    match self.verificar_checksum_disco(&destino, hash_esperado) {
                        Ok(true) => {
                            log::debug!("Checksum OK (en disco): {}", def.nombre_archivo);
                            continue;
                        }
                        Ok(false) => {
                            log::warn!(
                                "Checksum INVALIDO en disco para '{}'. Eliminando y re-descargando.",
                                def.nombre_archivo
                            );
                            let _ = fs::remove_file(&destino);
                            // Cae al bloque de descarga abajo
                        }
                        Err(e) => {
                            log::warn!(
                                "Error verificando checksum de '{}': {}",
                                def.nombre_archivo,
                                e
                            );
                            // Asumir corrupto y re-descargar
                            let _ = fs::remove_file(&destino);
                        }
                    }
                } else {
                    // Sin checksum definido: asumir que el archivo existente es valido.
                    continue;
                }
            }

            descargados += 1;
            if let Some(cb) = on_archivo {
                cb(def.nombre_archivo, descargados, total);
            }

            log::info!(
                "Descargando modelo [{}/{}]: {}/{}",
                descargados,
                total,
                def.categoria,
                def.nombre_archivo
            );
            self.descargar_con_reintentos(def, &destino, on_bytes)?;
        }

        if descargados > 0 {
            log::info!("{} modelo(s) descargados", descargados);
        } else {
            log::info!("Todos los modelos ya estan disponibles");
        }

        Ok(self.directorio_base.clone())
    }

    /// Reintentos ante fallos de red. Errores permanentes (404, checksum) no se reintentan.
    fn descargar_con_reintentos(
        &self,
        def: &ModelDefinition,
        destino: &Path,
        on_bytes: Option<&dyn Fn(u64, u64)>,
    ) -> Result<(), ModelDownloadError> {
        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }

        let temporal = destino.with_extension("tmp");
        let mut ultimo_error = ModelDownloadError::NetworkError("sin intentos".to_string());

        for intento in 1..=MAX_REINTENTOS {
            match self.descargar_streaming(def, &temporal, on_bytes) {
                Ok(hash) => {
                    match def.sha256_esperado {
                        Some(esperado) if hash != esperado => {
                            let _ = fs::remove_file(&temporal);
                            return Err(ModelDownloadError::IntegrityError {
                                expected: esperado.to_string(),
                                actual: hash,
                            });
                        }
                        None => log::info!(
                            "SHA256 de {}: {} (sin verificacion configurada)",
                            def.nombre_archivo,
                            hash
                        ),
                        _ => log::debug!("SHA256 verificado: {}", def.nombre_archivo),
                    }
                    fs::rename(&temporal, destino)?;
                    return Ok(());
                }
                Err(e) => {
                    if matches!(
                        e,
                        ModelDownloadError::NotFound(_) | ModelDownloadError::IntegrityError { .. }
                    ) {
                        return Err(e);
                    }
                    ultimo_error = e;
                    let _ = fs::remove_file(&temporal);
                    if intento < MAX_REINTENTOS {
                        log::warn!(
                            "Intento {}/{} fallido para {}: {}. Reintentando en {}s...",
                            intento,
                            MAX_REINTENTOS,
                            def.nombre_archivo,
                            ultimo_error,
                            ESPERA_REINTENTO_SECS
                        );
                        std::thread::sleep(std::time::Duration::from_secs(ESPERA_REINTENTO_SECS));
                    }
                }
            }
        }

        Err(ModelDownloadError::NetworkError(format!(
            "Fallaron {} intentos para {}: {}",
            MAX_REINTENTOS, def.nombre_archivo, ultimo_error
        )))
    }

    /// Streaming en chunks de 64 KB con SHA256 incremental.
    /// Llama `on_bytes` cada `CHUNKS_POR_REPORTE` chunks (~512 KB).
    fn descargar_streaming(
        &self,
        def: &ModelDefinition,
        temporal: &Path,
        on_bytes: Option<&dyn Fn(u64, u64)>,
    ) -> Result<String, ModelDownloadError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let respuesta = self
            .cliente_http
            .get(def.url)
            .send()
            .map_err(|e| ModelDownloadError::NetworkError(e.to_string()))?;

        if !respuesta.status().is_success() {
            return Err(ModelDownloadError::NotFound(format!(
                "{} (HTTP {})",
                def.url,
                respuesta.status()
            )));
        }

        let bytes_totales = respuesta.content_length().unwrap_or(0);
        log::info!(
            "Descargando {} ({:.1} MB)...",
            def.nombre_archivo,
            bytes_totales as f64 / 1_048_576.0
        );

        let mut archivo = fs::File::create(temporal)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut bytes_escritos: u64 = 0;
        let mut chunks_desde_reporte: usize = 0;
        let mut respuesta = respuesta;

        loop {
            let n = respuesta
                .read(&mut buffer)
                .map_err(|e| ModelDownloadError::NetworkError(e.to_string()))?;
            if n == 0 {
                break;
            }

            hasher.update(&buffer[..n]);
            archivo.write_all(&buffer[..n])?;
            bytes_escritos += n as u64;
            chunks_desde_reporte += 1;

            if chunks_desde_reporte >= CHUNKS_POR_REPORTE {
                if let Some(cb) = on_bytes {
                    cb(bytes_escritos, bytes_totales);
                }
                chunks_desde_reporte = 0;
            }
        }

        archivo.flush()?;

        // Reporte final con el total exacto
        if let Some(cb) = on_bytes {
            cb(bytes_escritos, bytes_escritos);
        }

        log::info!(
            "{} descargado ({:.1} MB)",
            def.nombre_archivo,
            bytes_escritos as f64 / 1_048_576.0
        );
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Indica si un artefacto nominal ya existe en el directorio local.
    pub fn modelo_existe(&self, categoria: &str, nombre_archivo: &str) -> bool {
        self.directorio_base
            .join(categoria)
            .join(nombre_archivo)
            .exists()
    }

    /// Construye la ruta esperada de un artefacto de modelo concreto.
    pub fn ruta_modelo(&self, categoria: &str, nombre_archivo: &str) -> PathBuf {
        self.directorio_base.join(categoria).join(nombre_archivo)
    }

    /// Indica si el conjunto completo de modelos requeridos ya está disponible.
    pub fn todos_los_modelos_disponibles(&self) -> bool {
        MODELOS_REQUERIDOS
            .iter()
            .all(|def| self.modelo_existe(def.categoria, def.nombre_archivo))
    }

    /// Lista los artefactos requeridos que aún no existen localmente.
    pub fn modelos_faltantes(&self) -> Vec<String> {
        MODELOS_REQUERIDOS
            .iter()
            .filter(|def| !self.modelo_existe(def.categoria, def.nombre_archivo))
            .map(|def| format!("{}/{}", def.categoria, def.nombre_archivo))
            .collect()
    }

    /// Calcula el SHA256 de un archivo en disco y lo compara con el hash esperado.
    ///
    /// Devuelve `Ok(true)` si el hash coincide, `Ok(false)` si no coincide,
    /// o `Err` si no se puede leer el archivo.
    fn verificar_checksum_disco(
        &self,
        ruta: &Path,
        hash_esperado: &str,
    ) -> Result<bool, ModelDownloadError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let mut archivo = fs::File::open(ruta)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK_SIZE];

        loop {
            let n = archivo
                .read(&mut buffer)
                .map_err(|e| ModelDownloadError::IoError(e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        let hash_actual = format!("{:x}", hasher.finalize());
        Ok(hash_actual == hash_esperado)
    }
}
