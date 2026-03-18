use ort::execution_providers::{CPUExecutionProvider, ExecutionProviderDispatch};
use std::sync::OnceLock;

static ORT_INIT: OnceLock<bool> = OnceLock::new();

/// Backend preferente compilado dentro del binario actual.
///
/// El enum modela capacidad compilada, no disponibilidad efectiva en runtime.
/// Esa distinción importa porque un binario puede incluir CUDA/TensorRT y aún así
/// caer a CPU si el host no satisface dependencias nativas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    #[cfg(feature = "tensorrt")]
    TensorRt,
    #[cfg(feature = "cuda")]
    Cuda,
    #[cfg(feature = "rocm")]
    Rocm,
    #[cfg(feature = "coreml")]
    CoreMl,
    Cpu,
}

impl Backend {
    /// Retorna el backend de mayor prioridad disponible en esta compilacion.
    #[allow(unreachable_code)]
    pub fn detectar() -> Self {
        #[cfg(feature = "tensorrt")]
        {
            return Self::TensorRt;
        }
        #[cfg(feature = "cuda")]
        {
            return Self::Cuda;
        }
        #[cfg(feature = "rocm")]
        {
            return Self::Rocm;
        }
        #[cfg(feature = "coreml")]
        {
            return Self::CoreMl;
        }
        Self::Cpu
    }

    /// Retorna el nombre estable del backend para logs y telemetría.
    pub fn nombre(&self) -> &'static str {
        match self {
            #[cfg(feature = "tensorrt")]
            Self::TensorRt => "TensorRT",
            #[cfg(feature = "cuda")]
            Self::Cuda => "CUDA",
            #[cfg(feature = "rocm")]
            Self::Rocm => "ROCm",
            #[cfg(feature = "coreml")]
            Self::CoreMl => "CoreML",
            Self::Cpu => "CPU",
        }
    }

    /// Indica si el backend compilado representa aceleración no-CPU.
    pub fn es_gpu(&self) -> bool {
        matches!(self, Self::Cpu) == false
    }
}

/// Retorna la cadena de `ExecutionProvider` ordenada por prioridad.
///
/// # Trade-offs
///
/// La lista termina siempre en CPU para preservar capacidad de arranque incluso
/// cuando la aceleración compilada no esté disponible en el host final.
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

/// Resultado observable del bootstrap de ONNX Runtime.
///
/// El tipo separa backend compilado, éxito de inicialización y si el backend es
/// realmente GPU para que UI y logs puedan distinguir capacidad, fallback y estado.
pub struct EstadoGpu {
    pub backend_compilado: &'static str,
    pub inicializado: bool,
    pub es_gpu: bool,
}

/// Inicializa ONNX Runtime una sola vez y retorna el estado efectivo.
///
/// `OnceLock` garantiza idempotencia global y evita carreras durante el arranque
/// del engine. La degradación a CPU es intencional para privilegiar disponibilidad
/// del producto frente a fallo duro por aceleración ausente.
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
        log::warn!(
            "GPU compilada ({}) pero init fallo. Usando CPU.",
            backend.nombre()
        );
    } else {
        log::info!("Ejecutando en CPU.");
    }

    estado
}

/// Heurística de paralelismo intra-op acorde al backend seleccionado.
///
/// # Performance
///
/// En GPU se reduce el número de threads de CPU para no competir con kernels ni
/// saturar el host; en CPU se usa una fracción de cores para evitar monopolizar
/// la máquina completa durante inferencia o preprocesamiento.
pub fn intra_threads(backend: Backend) -> usize {
    match backend {
        #[cfg(any(
            feature = "cuda",
            feature = "tensorrt",
            feature = "rocm",
            feature = "coreml"
        ))]
        b if b.es_gpu() => 2,
        _ => (std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            / 2)
        .max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_detectar_cpu_por_defecto() {
        let backend = Backend::detectar();
        assert_eq!(
            backend.nombre(),
            if cfg!(feature = "cuda") {
                "CUDA"
            } else if cfg!(feature = "tensorrt") {
                "TensorRT"
            } else if cfg!(feature = "rocm") {
                "ROCm"
            } else if cfg!(feature = "coreml") {
                "CoreML"
            } else {
                "CPU"
            }
        );
    }

    #[test]
    fn test_providers_siempre_incluye_cpu() {
        let providers = providers(0);
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
        assert_eq!(e1.inicializado, e2.inicializado);
        assert_eq!(e1.es_gpu, e2.es_gpu);
    }
}
