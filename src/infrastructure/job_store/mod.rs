//! Implementacion del almacenamiento de trabajos (jobs).
//!
//! Este modulo contiene las implementaciones concretas del puerto
//! `JobStorePort` para persistir y recuperar trabajos de procesamiento.

use crate::domain::errors::JobStoreError;
use crate::domain::{Job, JobStatus};
use crate::interfaces::ports::JobStorePort;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub use crate::interfaces::ports::JobStorePort as JobStore;

/// Almacen de trabajos en memoria.
///
/// Implementacion simple para desarrollo y pruebas.
/// Los datos se pierden al reiniciar la aplicacion.
pub struct InMemoryJobStore {
    jobs: Arc<RwLock<HashMap<String, Job>>>,
}

impl InMemoryJobStore {
    /// Crea un nuevo almacen en memoria vacio.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryJobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JobStorePort for InMemoryJobStore {
    fn save(&self, job: &Job) -> Result<(), JobStoreError> {
        let mut jobs = self
            .jobs
            .write()
            .map_err(|e| JobStoreError::LockError(e.to_string()))?;

        jobs.insert(job.id.clone(), job.clone());
        log::debug!("Trabajo guardado: {}", job.id);
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Job, JobStoreError> {
        let jobs = self
            .jobs
            .read()
            .map_err(|e| JobStoreError::LockError(e.to_string()))?;

        jobs.get(id)
            .cloned()
            .ok_or_else(|| JobStoreError::NotFound(id.to_string()))
    }

    fn update(&self, job: &Job) -> Result<(), JobStoreError> {
        let mut jobs = self
            .jobs
            .write()
            .map_err(|e| JobStoreError::LockError(e.to_string()))?;

        if jobs.contains_key(&job.id) {
            jobs.insert(job.id.clone(), job.clone());
            log::debug!("Trabajo actualizado: {}", job.id);
            Ok(())
        } else {
            Err(JobStoreError::NotFound(format!(
                "Trabajo no encontrado para actualizar: {}",
                job.id
            )))
        }
    }

    fn list(&self) -> Result<Vec<Job>, JobStoreError> {
        let jobs = self
            .jobs
            .read()
            .map_err(|e| JobStoreError::LockError(e.to_string()))?;

        Ok(jobs.values().cloned().collect())
    }

    fn delete(&self, id: &str) -> Result<(), JobStoreError> {
        let mut jobs = self
            .jobs
            .write()
            .map_err(|e| JobStoreError::LockError(e.to_string()))?;

        if jobs.remove(id).is_some() {
            log::debug!("Trabajo eliminado: {}", id);
            Ok(())
        } else {
            Err(JobStoreError::NotFound(id.to_string()))
        }
    }
}

/// Almacen de trabajos persistente en archivo JSON.
///
/// Serializa el mapa completo de jobs en `~/.local/share/ocrfast/jobs.json`.
/// Cada operacion de escritura es atomica: escribe en archivo temporal
/// y luego hace rename al destino final.
///
/// Los campos de imagen (`image_data`, `embedded_image`) no se persisten
/// para mantener el archivo compacto. Al cargar un job previo, los bloques
/// contienen el texto OCR extraido pero no las imagenes recortadas.
pub struct FileJobStore {
    ruta_archivo: PathBuf,
}

impl FileJobStore {
    /// Crea un almacen en la ruta por defecto: `~/.local/share/ocrfast/jobs.json`.
    pub fn new() -> Result<Self, JobStoreError> {
        let ruta_archivo = dirs::data_local_dir()
            .ok_or_else(|| {
                JobStoreError::PersistenceError(
                    "No se pudo obtener directorio local de datos".to_string(),
                )
            })?
            .join("ocrfast")
            .join("jobs.json");

        Ok(Self { ruta_archivo })
    }

    /// Crea un almacen en una ruta personalizada (util para tests).
    pub fn with_path(ruta: impl Into<PathBuf>) -> Self {
        Self { ruta_archivo: ruta.into() }
    }

    /// Carga el mapa completo de jobs desde disco.
    /// Retorna mapa vacio si el archivo no existe todavia.
    fn cargar(&self) -> Result<HashMap<String, Job>, JobStoreError> {
        if !self.ruta_archivo.exists() {
            return Ok(HashMap::new());
        }

        let contenido = fs::read_to_string(&self.ruta_archivo)
            .map_err(|e| JobStoreError::PersistenceError(format!("Error leyendo jobs: {}", e)))?;

        serde_json::from_str(&contenido)
            .map_err(|e| JobStoreError::PersistenceError(format!("Error parseando jobs.json: {}", e)))
    }

    /// Persiste el mapa completo de jobs en disco de forma atomica.
    fn persistir(&self, jobs: &HashMap<String, Job>) -> Result<(), JobStoreError> {
        // Garantizar que el directorio padre existe
        if let Some(directorio) = self.ruta_archivo.parent() {
            fs::create_dir_all(directorio).map_err(|e| {
                JobStoreError::PersistenceError(format!("Error creando directorio: {}", e))
            })?;
        }

        let json = serde_json::to_string_pretty(jobs)
            .map_err(|e| JobStoreError::PersistenceError(format!("Error serializando jobs: {}", e)))?;

        // Escritura atomica: temp + rename
        let ruta_temporal = self.ruta_archivo.with_extension("tmp");
        let mut archivo = fs::File::create(&ruta_temporal).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error creando archivo temporal: {}", e))
        })?;

        archivo.write_all(json.as_bytes()).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error escribiendo jobs: {}", e))
        })?;
        archivo.flush().map_err(|e| {
            JobStoreError::PersistenceError(format!("Error en flush: {}", e))
        })?;

        fs::rename(&ruta_temporal, &self.ruta_archivo).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error en rename atomico: {}", e))
        })?;

        log::debug!("Jobs persistidos en: {:?}", self.ruta_archivo);
        Ok(())
    }

    /// Retorna la ruta del archivo de almacenamiento.
    pub fn ruta(&self) -> &Path {
        &self.ruta_archivo
    }
}

impl Default for FileJobStore {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self::with_path("/tmp/ocrfast_jobs.json"))
    }
}

impl JobStorePort for FileJobStore {
    fn save(&self, job: &Job) -> Result<(), JobStoreError> {
        let mut jobs = self.cargar()?;
        jobs.insert(job.id.clone(), job.clone());
        self.persistir(&jobs)?;
        log::debug!("Job guardado en disco: {}", job.id);
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Job, JobStoreError> {
        let jobs = self.cargar()?;
        jobs.into_values()
            .find(|j| j.id == id)
            .ok_or_else(|| JobStoreError::NotFound(id.to_string()))
    }

    fn update(&self, job: &Job) -> Result<(), JobStoreError> {
        let mut jobs = self.cargar()?;
        if !jobs.contains_key(&job.id) {
            return Err(JobStoreError::NotFound(job.id.clone()));
        }
        jobs.insert(job.id.clone(), job.clone());
        self.persistir(&jobs)?;
        log::debug!("Job actualizado en disco: {}", job.id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Job>, JobStoreError> {
        let mut jobs: Vec<Job> = self.cargar()?.into_values().collect();
        // Ordenar por fecha de creacion para orden determinista en la UI
        jobs.sort_by_key(|j| j.created_at);
        Ok(jobs)
    }

    fn delete(&self, id: &str) -> Result<(), JobStoreError> {
        let mut jobs = self.cargar()?;
        if jobs.remove(id).is_none() {
            return Err(JobStoreError::NotFound(id.to_string()));
        }
        self.persistir(&jobs)?;
        log::debug!("Job eliminado de disco: {}", id);
        Ok(())
    }
}

/// Normaliza jobs cargados desde disco para consistencia al arrancar.
///
/// Los jobs en estado `Processing` al momento del cierre quedaron
/// interrumpidos sin completarse. Se marcan como `Failed` para que
/// el usuario pueda reprocesarlos si lo desea.
pub fn normalizar_jobs_al_arranque(jobs: &mut Vec<Job>) {
    for job in jobs.iter_mut() {
        if job.status == JobStatus::Processing {
            job.status = JobStatus::Failed;
            job.error_message = Some(
                "Interrumpido: la aplicacion se cerro durante el procesamiento".to_string(),
            );
        }
    }
}
