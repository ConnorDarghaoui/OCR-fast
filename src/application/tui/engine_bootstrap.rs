use crate::interfaces::ports::OcrEnginePort;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Eventos emitidos por el hilo de carga del motor OCR.
///
/// El enum se mantiene privado al coordinador para que `AppState` no tenga que
/// conocer detalles del canal ni del protocolo entre threads.
enum MotorCargaEvento {
    Descargando {
        nombre: String,
        actual: usize,
        total: usize,
    },
    DescargandoBytes {
        bytes_actual: u64,
        bytes_total: u64,
    },
    GpuInfo {
        backend: String,
        activo: bool,
    },
    Listo(Arc<dyn OcrEnginePort>),
    Error(String),
}

/// Resultado de drenar un tick de bootstrap del motor.
pub(crate) struct EngineBootstrapTransition {
    pub(crate) motor: Option<Arc<dyn OcrEnginePort>>,
    pub(crate) mensaje_log: Option<String>,
    pub(crate) mensaje_estado: Option<String>,
    pub(crate) abrir_lista_trabajos: bool,
}

/// Estado y coordinación del bootstrap de motor OCR en background.
///
/// Este componente encapsula el canal, el progreso visible y la traducción de
/// eventos técnicos a estado consumible por la TUI. `AppState` solo necesita
/// decidir qué hacer con el engine listo y con los mensajes de feedback.
pub(crate) struct EngineBootstrapState {
    receptor: Option<Receiver<MotorCargaEvento>>,
    motor_cargado: bool,
    motor_fallido: bool,
    progreso: f32,
    fase: String,
    bytes_actual: u64,
    bytes_total: u64,
    gpu_info: String,
}

impl EngineBootstrapState {
    /// Construye el estado inicial del bootstrap de motor.
    pub(crate) fn new() -> Self {
        Self {
            receptor: None,
            motor_cargado: false,
            motor_fallido: false,
            progreso: 0.0,
            fase: "Verificando modelos...".to_string(),
            bytes_actual: 0,
            bytes_total: 0,
            gpu_info: String::new(),
        }
    }

    /// Marca el motor como listo cuando el caller decide omitir la carga ONNX.
    pub(crate) fn marcar_listo_sin_carga(&mut self) {
        self.motor_cargado = true;
        self.progreso = 1.0;
        self.fase = "Motor listo".to_string();
        self.receptor = None;
    }

    /// Inicia el bootstrap ONNX en un hilo de fondo.
    pub(crate) fn iniciar_carga(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<MotorCargaEvento>();
        self.receptor = Some(rx);
        self.progreso = 0.0;
        self.fase = "Verificando modelos...".to_string();
        self.bytes_actual = 0;
        self.bytes_total = 0;

        std::thread::spawn(move || {
            use crate::infrastructure::ocr_engines::onnx::{
                gpu_config, ModelDownloader, OnnxOcrEngine,
            };

            let estado_gpu = gpu_config::inicializar(0);
            let _ = tx.send(MotorCargaEvento::GpuInfo {
                backend: estado_gpu.backend_compilado.to_string(),
                activo: estado_gpu.inicializado && estado_gpu.es_gpu,
            });

            let downloader = match ModelDownloader::new() {
                Ok(downloader) => downloader,
                Err(error) => {
                    let _ = tx.send(MotorCargaEvento::Error(error.to_string()));
                    return;
                }
            };

            let tx_archivo = tx.clone();
            let on_archivo = |nombre: &str, actual: usize, total: usize| {
                let _ = tx_archivo.send(MotorCargaEvento::Descargando {
                    nombre: nombre.to_string(),
                    actual,
                    total,
                });
            };

            let tx_bytes = tx.clone();
            let on_bytes = |bytes_actual: u64, bytes_total: u64| {
                let _ = tx_bytes.send(MotorCargaEvento::DescargandoBytes {
                    bytes_actual,
                    bytes_total,
                });
            };

            let ruta_modelos =
                match downloader.asegurar_todos_los_modelos(Some(&on_archivo), Some(&on_bytes)) {
                    Ok(ruta) => ruta,
                    Err(error) => {
                        let _ = tx.send(MotorCargaEvento::Error(error.to_string()));
                        return;
                    }
                };

            match OnnxOcrEngine::from_directory(&ruta_modelos) {
                Ok(engine) => {
                    let _ = tx.send(MotorCargaEvento::Listo(Arc::new(engine)));
                }
                Err(error) => {
                    let _ = tx.send(MotorCargaEvento::Error(error.to_string()));
                }
            }
        });
    }

    /// Procesa un único evento pendiente de bootstrap si existe.
    pub(crate) fn drenar_evento(&mut self) -> Option<EngineBootstrapTransition> {
        let evento = match &self.receptor {
            Some(receiver) => match receiver.try_recv() {
                Ok(evento) => evento,
                Err(_) => return None,
            },
            None => return None,
        };

        let mut transicion = EngineBootstrapTransition {
            motor: None,
            mensaje_log: None,
            mensaje_estado: None,
            abrir_lista_trabajos: false,
        };

        match evento {
            MotorCargaEvento::GpuInfo { backend, activo } => {
                self.gpu_info = if activo {
                    format!("GPU: {} (activa)", backend)
                } else if backend == "CPU" {
                    "CPU (sin aceleracion GPU)".to_string()
                } else {
                    format!("GPU: {} (no disponible, usando CPU)", backend)
                };
                transicion.mensaje_log = Some(format!("Aceleracion: {}", self.gpu_info));
            }
            MotorCargaEvento::Descargando {
                nombre,
                actual,
                total,
            } => {
                self.progreso = actual as f32 / total as f32;
                self.fase = format!("Descargando {} ({}/{})", nombre, actual, total);
                self.bytes_actual = 0;
                self.bytes_total = 0;
                transicion.mensaje_log = Some(format!(
                    "Descargando modelo {}/{}: {}",
                    actual, total, nombre
                ));
            }
            MotorCargaEvento::DescargandoBytes {
                bytes_actual,
                bytes_total,
            } => {
                self.bytes_actual = bytes_actual;
                self.bytes_total = bytes_total;
            }
            MotorCargaEvento::Listo(motor) => {
                self.motor_cargado = true;
                self.progreso = 1.0;
                self.receptor = None;
                transicion.motor = Some(motor);
                transicion.mensaje_log = Some("Motor OCR ONNX cargado exitosamente".to_string());
                transicion.mensaje_estado = Some("Motor OCR listo".to_string());
                transicion.abrir_lista_trabajos = true;
            }
            MotorCargaEvento::Error(error) => {
                self.motor_cargado = true;
                self.motor_fallido = true;
                self.receptor = None;
                transicion.mensaje_log = Some(format!(
                    "Error motor ONNX: {}. Usando Stub (resultados ficticios).",
                    error
                ));
                transicion.mensaje_estado = Some(format!("Error motor ONNX: {}", error));
                transicion.abrir_lista_trabajos = true;
            }
        }

        Some(transicion)
    }

    /// Indica si el motor ya dejó de estar en bootstrap.
    pub(crate) fn motor_cargado(&self) -> bool {
        self.motor_cargado
    }

    /// Indica si el bootstrap falló y la app quedó degradada.
    pub(crate) fn motor_fallido(&self) -> bool {
        self.motor_fallido
    }

    /// Retorna el progreso agregado de bootstrap.
    pub(crate) fn progreso(&self) -> f32 {
        self.progreso
    }

    /// Retorna la fase visible de bootstrap.
    pub(crate) fn fase(&self) -> &str {
        &self.fase
    }

    /// Retorna los bytes descargados del archivo actual.
    pub(crate) fn bytes_actual(&self) -> u64 {
        self.bytes_actual
    }

    /// Retorna el tamaño total del archivo actual si se conoce.
    pub(crate) fn bytes_total(&self) -> u64 {
        self.bytes_total
    }

    /// Retorna la descripción visible del backend ONNX efectivo.
    pub(crate) fn gpu_info(&self) -> &str {
        &self.gpu_info
    }
}
