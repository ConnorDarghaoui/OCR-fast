//! Configuracion de aceleracion GPU para sesiones ONNX Runtime.
//!
//! Detecta y configura el mejor Execution Provider disponible en tiempo de
//! compilacion y ejecucion. El orden de preferencia es:
//!   TensorRT > CUDA > ROCm > CoreML > CPU
//!
//! Uso:
//!   cargo build --features cuda      # NVIDIA (CUDA)
//!   cargo build --features tensorrt  # NVIDIA (TensorRT, mas rapido)
//!   cargo build --features rocm      # AMD
//!   cargo build --features coreml    # Apple Silicon
//!   cargo build                      # CPU (siempre disponible)

use ort::execution_providers::{CPUExecutionProvider, ExecutionProviderDispatch};
use std::sync::OnceLock;

static ORT_INIT: OnceLock<bool> = OnceLock::new();

/// Backend de aceleracion activo en esta compilacion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    #[cfg(feature = "tensorrt")] TensorRt,
    #[cfg(feature = "cuda")]     Cuda,
    #[cfg(feature = "rocm")]     Rocm,
    #[cfg(feature = "coreml")]   CoreMl,
    Cpu,
}

impl Backend {
    /// Retorna el backend de mayor prioridad disponible en esta compilacion.
    #[allow(unreachable_code)]
    pub fn detectar() -> Self {
        #[cfg(feature = "tensorrt")] { return Self::TensorRt; }
        #[cfg(feature = "cuda")]     { return Self::Cuda; }
        #[cfg(feature = "rocm")]     { return Self::Rocm; }
        #[cfg(feature = "coreml")]   { return Self::CoreMl; }
        Self::Cpu
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            #[cfg(feature = "tensorrt")] Self::TensorRt => "TensorRT",
            #[cfg(feature = "cuda")]     Self::Cuda     => "CUDA",
            #[cfg(feature = "rocm")]     Self::Rocm     => "ROCm",
            #[cfg(feature = "coreml")]   Self::CoreMl   => "CoreML",
            Self::Cpu                                   => "CPU",
        }
    }

    pub fn es_gpu(&self) -> bool {
        matches!(self, Self::Cpu) == false
    }
}

/// Retorna la lista de Execution Providers ordenada por prioridad.
///
/// ort intenta cada EP en orden y usa el primero que funciona,
/// cayendo siempre a CPU si ninguno esta disponible en runtime.
pub fn providers(_device_id: i32) -> Vec<ExecutionProviderDispatch> {
    let mut providers: Vec<ExecutionProviderDispatch> = Vec::new();

    #[cfg(feature = "tensorrt")]
    {
        use ort::execution_providers::TensorRTExecutionProvider;
        providers.push(
            TensorRTExecutionProvider::default()
                .with_device_id(device_id)
                .build(),
        );
    }

    #[cfg(feature = "cuda")]
    {
        use ort::execution_providers::CUDAExecutionProvider;
        providers.push(
            CUDAExecutionProvider::default()
                .with_device_id(device_id)
                .build(),
        );
    }

    #[cfg(feature = "rocm")]
    {
        use ort::execution_providers::ROCmExecutionProvider;
        providers.push(
            ROCmExecutionProvider::default()
                .with_device_id(device_id)
                .build(),
        );
    }

    #[cfg(feature = "coreml")]
    {
        use ort::execution_providers::CoreMLExecutionProvider;
        providers.push(CoreMLExecutionProvider::default().build());
    }

    providers.push(CPUExecutionProvider::default().build());

    providers
}

/// Estado real de la GPU tras intentar inicializar ONNX Runtime.
pub struct EstadoGpu {
    pub backend_compilado: &'static str,
    pub inicializado: bool,
    pub es_gpu: bool,
}

/// Configura ONNX Runtime globalmente con el mejor EP disponible y retorna el estado real.
///
/// Debe llamarse una vez antes de crear cualquier sesion ONNX.
/// Si la configuracion GPU falla, cae silenciosamente a CPU.
pub fn inicializar(device_id: i32) -> EstadoGpu {
    let backend = Backend::detectar();
    let inicializado = *ORT_INIT.get_or_init(|| {
        ort::init()
            .with_name("ocrfast")
            .with_execution_providers(providers(device_id))
            .commit()
    });

    let estado = EstadoGpu {
        backend_compilado: backend.nombre(),
        inicializado,
        es_gpu: backend.es_gpu(),
    };

    if estado.inicializado && estado.es_gpu {
        log::info!("GPU activa: {} (device {})", backend.nombre(), device_id);
    } else if estado.es_gpu {
        log::warn!("GPU compilada ({}) pero init fallo. Usando CPU.", backend.nombre());
    } else {
        log::info!("Ejecutando en CPU.");
    }

    estado
}

/// Numero de threads intra-op optimo segun el backend.
///
/// En GPU los threads de CPU son menos criticos; en CPU conviene
/// usar todos los nucleos disponibles para el preprocesamiento.
pub fn intra_threads(backend: Backend) -> usize {
    match backend {
        #[cfg(any(feature = "cuda", feature = "tensorrt", feature = "rocm", feature = "coreml"))]
        b if b.es_gpu() => 2,
        _ => {
            // Mitad de los nucleos logicos para no saturar el sistema
            (std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4) / 2)
                .max(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_detectar_cpu_por_defecto() {
        // Sin features GPU compiladas, debe retornar Cpu
        let backend = Backend::detectar();
        // En CI sin GPU features: debe ser Cpu
        assert_eq!(backend.nombre(), if cfg!(feature = "cuda") { "CUDA" }
            else if cfg!(feature = "tensorrt") { "TensorRT" }
            else if cfg!(feature = "rocm") { "ROCm" }
            else if cfg!(feature = "coreml") { "CoreML" }
            else { "CPU" });
    }

    #[test]
    fn test_providers_siempre_incluye_cpu() {
        let providers = providers(0);
        // Debe haber al menos 1 provider (CPU)
        assert!(!providers.is_empty());
    }

    #[test]
    fn test_intra_threads_retorna_al_menos_1() {
        let backend = Backend::detectar();
        assert!(intra_threads(backend) >= 1);
    }

    #[test]
    fn test_inicializar_retorna_estado_coherente() {
        let estado = inicializar(0);
        assert!(!estado.backend_compilado.is_empty());
        // Sin features GPU: backend es CPU, es_gpu = false
        if !estado.es_gpu {
            assert_eq!(estado.backend_compilado, "CPU");
            assert!(estado.inicializado, "CPU debe inicializar siempre");
        }
    }

    #[test]
    fn test_inicializar_es_idempotente() {
        let e1 = inicializar(0);
        let e2 = inicializar(0);
        assert_eq!(e1.backend_compilado, e2.backend_compilado);
        assert_eq!(e1.inicializado,      e2.inicializado);
        assert_eq!(e1.es_gpu,            e2.es_gpu);
    }
}
