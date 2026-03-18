use crate::domain::errors::ModelDownloadError;
use crate::infrastructure::ocr_engines::onnx::gpu_config::{self, EstadoGpu};
use crate::infrastructure::ocr_engines::onnx::model_downloader::ModelDownloader;
use std::path::{Path, PathBuf};

/// Resultado estable del aprovisionamiento previo al engine ONNX.
///
/// Este valor encapsula los dos artefactos que el engine necesita recibir ya
/// resueltos desde afuera: el estado efectivo de ONNX Runtime y la ubicación del
/// árbol de modelos listo para cargar sesiones.
pub struct ProvisionedOnnxRuntime {
    estado_gpu: EstadoGpu,
    ruta_modelos: PathBuf,
}

impl ProvisionedOnnxRuntime {
    /// Construye un runtime aprovisionado a partir del estado GPU y la ruta de modelos.
    pub fn new(estado_gpu: EstadoGpu, ruta_modelos: PathBuf) -> Self {
        Self {
            estado_gpu,
            ruta_modelos,
        }
    }

    /// Retorna el estado efectivo del bootstrap de ONNX Runtime.
    pub fn estado_gpu(&self) -> &EstadoGpu {
        &self.estado_gpu
    }

    /// Retorna la ruta del directorio que contiene todos los modelos requeridos.
    pub fn ruta_modelos(&self) -> &Path {
        &self.ruta_modelos
    }
}

/// Orquestador de bootstrap para runtime ONNX y artefactos de modelos.
///
/// El provisioner concentra política operativa: inicialización global de runtime,
/// localización del cache de modelos, descarga bajo demanda y callbacks de
/// progreso. Separar esta responsabilidad evita que `OnnxOcrEngine` combine
/// inferencia con red, filesystem y decisiones de aprovisionamiento.
pub struct ModelRuntimeProvisioner {
    downloader: ModelDownloader,
    device_id: i32,
}

impl ModelRuntimeProvisioner {
    /// Construye el provisioner con el downloader por defecto y `device_id = 0`.
    pub fn new() -> Result<Self, ModelDownloadError> {
        let downloader = ModelDownloader::new()?;
        Ok(Self::with_downloader(downloader))
    }

    /// Construye el provisioner a partir de un downloader ya configurado.
    pub fn with_downloader(downloader: ModelDownloader) -> Self {
        Self {
            downloader,
            device_id: 0,
        }
    }

    /// Permite elegir el dispositivo preferido antes del bootstrap.
    pub fn with_device_id(mut self, device_id: i32) -> Self {
        self.device_id = device_id;
        self
    }

    /// Expone la ruta base del cache de modelos que este provisioner administrará.
    pub fn directorio_modelos(&self) -> &Path {
        self.downloader.directorio_base()
    }

    /// Inicializa runtime y garantiza que los modelos requeridos estén listos.
    ///
    /// # Errors
    ///
    /// Retorna `ModelDownloadError` si no puede resolverse el directorio base o
    /// si la adquisición de modelos falla por red, integridad o filesystem.
    pub fn provision(
        &self,
        on_gpu: Option<&dyn Fn(&EstadoGpu)>,
        on_archivo: Option<&dyn Fn(&str, usize, usize)>,
        on_bytes: Option<&dyn Fn(u64, u64)>,
    ) -> Result<ProvisionedOnnxRuntime, ModelDownloadError> {
        let estado_gpu = gpu_config::inicializar(self.device_id);

        if let Some(callback) = on_gpu {
            callback(&estado_gpu);
        }

        let ruta_modelos = self
            .downloader
            .asegurar_todos_los_modelos(on_archivo, on_bytes)?;

        Ok(ProvisionedOnnxRuntime::new(estado_gpu, ruta_modelos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provisioner_reutiliza_directorio_configurado() {
        let ruta =
            std::env::temp_dir().join(format!("ocrfast_provisioner_test_{}", uuid::Uuid::new_v4()));
        let downloader = ModelDownloader::with_directory(ruta.clone()).unwrap();
        let provisioner = ModelRuntimeProvisioner::with_downloader(downloader);

        assert_eq!(provisioner.directorio_modelos(), ruta.as_path());
    }
}
