use crate::domain::errors::JobStoreError;
use crate::domain::{Job, JobStatus};
use crate::interfaces::ports::JobStorePort;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Alias público del puerto de almacenamiento para compatibilidad histórica.
pub use crate::interfaces::ports::JobStorePort as JobStore;

/// Implementación de almacenamiento volátil respaldada por `RwLock`.
///
/// Se usa para pruebas, entornos efímeros y fallback cuando la persistencia en
/// disco no es deseable. El `RwLock` permite muchas lecturas concurrentes con
/// escrituras exclusivas y mantiene el coste de coordinación bajo para el tamaño
/// esperado del estado.
pub struct InMemoryJobStore {
    jobs: Arc<RwLock<HashMap<String, Job>>>,
}

impl InMemoryJobStore {
    /// Crea un almacén en memoria vacío.
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

/// Almacén persistente de jobs respaldado por un archivo JSON local.
///
/// El store serializa snapshots completos porque el volumen esperado es pequeño y
/// ese enfoque simplifica recuperación, depuración y compatibilidad. Las
/// escrituras usan temp + rename para minimizar ventanas de corrupción visible.
///
/// # Trade-offs
///
/// Reescribir el mapa completo en cada mutación no escalaría a miles de jobs,
/// pero reduce complejidad operativa y evita un formato incremental más frágil.
pub struct FileJobStore {
    ruta_archivo: PathBuf,
}

impl FileJobStore {
    /// Crea un store persistente en la ruta local estándar de OCRFast.
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

    /// Crea un store en una ruta explícita útil para pruebas o embedding.
    pub fn with_path(ruta: impl Into<PathBuf>) -> Self {
        Self {
            ruta_archivo: ruta.into(),
        }
    }

    /// Carga el mapa completo de jobs desde disco.
    /// Retorna mapa vacio si el archivo no existe todavia.
    fn cargar(&self) -> Result<HashMap<String, Job>, JobStoreError> {
        if !self.ruta_archivo.exists() {
            return Ok(HashMap::new());
        }

        let contenido = fs::read_to_string(&self.ruta_archivo)
            .map_err(|e| JobStoreError::PersistenceError(format!("Error leyendo jobs: {}", e)))?;

        serde_json::from_str(&contenido).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error parseando jobs.json: {}", e))
        })
    }

    /// Persiste el mapa completo de jobs en disco de forma atomica.
    fn persistir(&self, jobs: &HashMap<String, Job>) -> Result<(), JobStoreError> {
        // Garantizar que el directorio padre existe
        if let Some(directorio) = self.ruta_archivo.parent() {
            fs::create_dir_all(directorio).map_err(|e| {
                JobStoreError::PersistenceError(format!("Error creando directorio: {}", e))
            })?;
        }

        let json = serde_json::to_string_pretty(jobs).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error serializando jobs: {}", e))
        })?;

        // Escritura atomica: temp + rename
        let ruta_temporal = self.ruta_archivo.with_extension("tmp");
        let mut archivo = fs::File::create(&ruta_temporal).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error creando archivo temporal: {}", e))
        })?;

        archivo.write_all(json.as_bytes()).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error escribiendo jobs: {}", e))
        })?;
        archivo
            .flush()
            .map_err(|e| JobStoreError::PersistenceError(format!("Error en flush: {}", e)))?;

        fs::rename(&ruta_temporal, &self.ruta_archivo).map_err(|e| {
            JobStoreError::PersistenceError(format!("Error en rename atomico: {}", e))
        })?;

        log::debug!("Jobs persistidos en: {:?}", self.ruta_archivo);
        Ok(())
    }

    /// Retorna la ruta física del archivo de almacenamiento.
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

/// Repara snapshots persistidos que quedaron en un estado imposible al reiniciar.
///
/// Un job no puede seguir en `Processing` tras un relanzamiento porque el worker
/// original ya no existe. La normalización convierte ese estado en `Failed` con
/// un mensaje explícito para mantener consistencia operativa y evitar UI engañosa.
pub fn normalizar_jobs_al_arranque(jobs: &mut Vec<Job>) {
    for job in jobs.iter_mut() {
        if job.status == JobStatus::Processing {
            job.status = JobStatus::Failed;
            job.error_message =
                Some("Interrumpido: la aplicacion se cerro durante el procesamiento".to_string());
        }
    }
}
