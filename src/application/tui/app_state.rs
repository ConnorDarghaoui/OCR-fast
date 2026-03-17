//! Estado global de la TUI y unico punto que lanza threads de procesamiento.
//!
//! Cada job corre en su propio thread y se comunica via `mpsc`. Ningun otro
//! modulo de la TUI gestiona canales directamente.

use crate::application::pipeline::{OcrPipeline, PipelineEvent, MSG_JOB_CANCELADO};
use crate::domain::{Job, JobStatus, LanguageConfig, OutputFormat, ProcessingProfile};
use crate::infrastructure::exporters::{JsonExporter, MarkdownExporter, PdfSandwichExporter};
use crate::infrastructure::job_store::normalizar_jobs_al_arranque;
use crate::infrastructure::postprocessors::TextPostprocessor;
use crate::interfaces::ports::{DocumentParserPort, ExporterPort, JobStorePort, OcrEnginePort};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Vista actual de la aplicacion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Pantalla de carga inicial (modelos).
    Initializing,
    /// Lista de trabajos (vista principal).
    JobList,
    /// Detalles de un trabajo especifico.
    JobDetail,
    /// Configuracion de la aplicacion.
    Settings,
    /// Pantalla de ayuda con atajos de teclado.
    Help,
}

/// [COMPONENTE] Enumeracion de estado de interaccion TUI.
///
/// [PROPÓSITO] Discretizar la modalidad de recepcion de eventos del teclado,
/// aislando los comandos de navegacion global respecto de la captura de texto crudo en inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Modo normal: comandos vim-like y navegacion de componentes.
    Normal,
    /// Modo de edicion: buffer inyectable para captura de multiples caracteres.
    Editing,
}

/// [COMPONENTE] Contrato de eventos de hilo (Background -> TUI).
///
/// [PROPÓSITO] Actuar como capa DTO para la sincronizacion asincrona del estado
/// de inicializacion de modelos AI (que es I/O y CPU intensivo), previniendo el bloqueo
/// del event loop de pintado de la terminal.
pub enum MotorCargaEvento {
    /// Transmite la metadata del bloque de descarga instanciado hacia el sistema log.
    Descargando { nombre: String, actual: usize, total: usize },
    /// Tasa de actualizacion de telemetria en bytes transferidos puros.
    DescargandoBytes { bytes_actual: u64, bytes_total: u64 },
    /// Informacion de la capa de acceleracion instanciada.
    GpuInfo { backend: String, activo: bool },
    /// Contenedor de traslacion del port en memoria compartida (exitoso).
    Listo(Arc<dyn OcrEnginePort>),
    /// Traslado del error en inicializacion del Motor impidiendo uso.
    Error(String),
}

/// [COMPONENTE] Estructura de Memoria Central de Interfaz.
///
/// [PROPÓSITO] Preservar la unica fuente de verdad (Single Source of Truth) para 
/// todos los datos renderizables de la terminal. Actua a su vez como controlador maestro
/// asincrono almacenando los descriptores TX/RX de los trabajos derivados al pool the workers.
pub struct AppState {
    pub trabajos: Vec<Job>,
    pub indice_seleccionado: usize,
    pub vista_actual: ViewMode,
    pub modo_entrada: InputMode,
    pub buffer_entrada: String,
    pub perfil: ProcessingProfile,
    pub idioma: LanguageConfig,
    pub debe_salir: bool,
    pub analizador_documentos: Arc<dyn DocumentParserPort>,
    pub motor_ocr: Arc<dyn OcrEnginePort>,
    job_store: Arc<dyn JobStorePort>,
    /// `false` mientras el motor ONNX no haya llegado; bloquea la creacion de jobs.
    pub motor_cargado: bool,
    /// `true` si ONNX fallo y el stub quedo activo. La TUI muestra banner de advertencia.
    pub motor_fallido: bool,
    /// Receptores de eventos de pipeline activos (job_id -> receiver).
    receptores_activos: HashMap<String, Receiver<PipelineEvent>>,
    /// Receptor para eventos del motor OCR durante la carga en background.
    receptor_motor: Option<Receiver<MotorCargaEvento>>,
    /// Fase actual del pipeline por job (job_id -> descripcion de fase).
    pub fase_actual: HashMap<String, String>,
    /// Progreso actual del pipeline por job (job_id -> progreso 0.0-1.0).
    pub progreso_actual: HashMap<String, f32>,
    /// Scroll vertical en la vista de detalle.
    pub scroll_detalle: u16,
    /// Mensaje temporal de status (feedback al usuario).
    pub mensaje_estado: Option<(String, std::time::Instant)>,
    /// Historial de eventos del sistema para el panel de logs.
    pub registros: VecDeque<String>,
    /// Progreso de la carga inicial de modelos (0.0-1.0), basado en archivos.
    pub progreso_carga_motor: f32,
    /// Descripcion de la fase actual de carga del motor.
    pub fase_carga_motor: String,
    /// Bytes descargados del archivo actual.
    pub bytes_carga_actual: u64,
    /// Tamaño total del archivo actual (0 si desconocido).
    pub bytes_carga_total: u64,
    /// Descripcion del estado GPU para mostrar en la TUI.
    pub gpu_info: String,
    /// Flags de cancelacion por job_id. Activar a `true` cancela el job.
    cancelaciones_activas: HashMap<String, Arc<AtomicBool>>,
    /// True cuando se espera que el usuario elija el formato de salida.
    pub seleccionando_formato: bool,
    /// Ruta pendiente mientras se selecciona el formato.
    pub ruta_pendiente: Option<String>,
    /// Indice del formato seleccionado en la lista de opciones.
    pub indice_formato: usize,
}

impl AppState {
    /// Crea un nuevo estado de la aplicacion con dependencias inyectadas.
    ///
    /// Carga los trabajos previos desde el `job_store` al arrancar.
    /// Los trabajos que quedaron en estado `Processing` (sesion interrumpida)
    /// se marcan automaticamente como `Failed`.
    pub fn nuevo(
        analizador_documentos: Arc<dyn DocumentParserPort>,
        motor_ocr: Arc<dyn OcrEnginePort>,
        job_store: Arc<dyn JobStorePort>,
    ) -> Self {
        let (trabajos, log_arranque) = cargar_trabajos_iniciales(&*job_store);

        let mut estado = Self {
            trabajos,
            indice_seleccionado: 0,
            vista_actual: ViewMode::Initializing,
            modo_entrada: InputMode::Normal,
            buffer_entrada: String::new(),
            perfil: ProcessingProfile::default(),
            idioma: LanguageConfig::default(),
            debe_salir: false,
            analizador_documentos,
            motor_ocr,
            job_store,
            motor_cargado: false,
            motor_fallido: false,
            receptores_activos: HashMap::new(),
            receptor_motor: None,
            fase_actual: HashMap::new(),
            progreso_actual: HashMap::new(),
            scroll_detalle: 0,
            mensaje_estado: None,
            registros: VecDeque::from(["Sistema inicializado...".to_string()]),
            progreso_carga_motor: 0.0,
            fase_carga_motor: "Verificando modelos...".to_string(),
            bytes_carga_actual: 0,
            bytes_carga_total: 0,
            gpu_info: String::new(),
            cancelaciones_activas: HashMap::new(),
            seleccionando_formato: false,
            ruta_pendiente: None,
            indice_formato: 0,
        };

        for mensaje in log_arranque {
            estado.loguear(mensaje);
        }

        estado
    }

    /// Agrega un log al sistema con timestamp.
    pub fn loguear(&mut self, mensaje: String) {
        const MAX_REGISTROS: usize = 100;
        let marca_tiempo = chrono::Local::now().format("%H:%M:%S").to_string();
        self.registros.push_back(format!("[{}] {}", marca_tiempo, mensaje));
        if self.registros.len() > MAX_REGISTROS {
            self.registros.pop_front();
        }
    }

    /// Selecciona el siguiente trabajo en la lista.
    pub fn seleccionar_siguiente(&mut self) {
        if !self.trabajos.is_empty() {
            self.indice_seleccionado = (self.indice_seleccionado + 1) % self.trabajos.len();
        }
    }

    /// Selecciona el trabajo anterior en la lista.
    pub fn seleccionar_anterior(&mut self) {
        if !self.trabajos.is_empty() {
            if self.indice_seleccionado > 0 {
                self.indice_seleccionado -= 1;
            } else {
                self.indice_seleccionado = self.trabajos.len() - 1;
            }
        }
    }

    /// Obtiene el trabajo seleccionado actualmente.
    pub fn obtener_trabajo_seleccionado(&self) -> Option<&Job> {
        self.trabajos.get(self.indice_seleccionado)
    }

    /// Cambia a modo de edicion para agregar un nuevo archivo.
    pub fn iniciar_agregar_archivo(&mut self) {
        self.modo_entrada = InputMode::Editing;
        self.buffer_entrada.clear();
    }

    /// Cancela la edicion y vuelve a modo normal.
    pub fn cancelar_edicion(&mut self) {
        self.modo_entrada = InputMode::Normal;
        self.buffer_entrada.clear();
    }

    /// [COMPONENTE] Controlador de validacion de entrada de archivos.
    ///
    /// [PROPÓSITO] Validar el input del usuario garantizando que el archivo existe
    /// y posee una extension soportada antes de transicionar al estado de seleccion de formato.
    ///
    /// [ENTRADAS] Ninguna, consume desde el estado interno (`buffer_entrada`).
    ///
    /// [FLUJO DE EJECUCIÓN] 
    /// |> Verifica que el buffer no este vacio.
    /// |> Delega al sistema operativo la verificacion de existencia y tipo de archivo.
    /// |> Extrae y normaliza la extension para validarla contra la lista de extensiones permitidas.
    /// |> Muta el estado interno para persistir la ruta pendiente, restaurar modos de interfaz y habilitar la seleccion de formato.
    ///
    /// [RESULTADO] `Result<(), String>`: Transicion de estado exitosa.
    ///
    /// [EXCEPCIONES] Retorna `Err` explicito si la ruta esta vacia, el archivo es inaccesible, o el formato no es util para OCR.
    pub fn procesar_archivo_ingresado(&mut self) -> Result<(), String> {
        if self.buffer_entrada.is_empty() {
            return Err("Ruta de archivo vacia".to_string());
        }
        let ruta = self.buffer_entrada.clone();

        let meta = std::fs::metadata(&ruta)
            .map_err(|_| format!("Archivo no encontrado: {}", ruta))?;
        if !meta.is_file() {
            return Err(format!("La ruta no es un archivo: {}", ruta));
        }

        const EXTENSIONES_SOPORTADAS: &[&str] = &["png", "jpg", "jpeg", "tiff", "tif", "pdf"];
        let ext = std::path::Path::new(&ruta)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !EXTENSIONES_SOPORTADAS.contains(&ext.as_str()) {
            return Err(format!(
                "Formato no soportado: .{} (soportados: PNG, JPEG, TIFF, PDF)",
                ext
            ));
        }

        self.ruta_pendiente = Some(ruta);
        self.buffer_entrada.clear();
        self.modo_entrada = InputMode::Normal;
        self.seleccionando_formato = true;
        Ok(())
    }

    /// [COMPONENTE] Metodo de confirmacion de seleccion.
    ///
    /// [PROPÓSITO] Extraer el estado pendiente seleccionado por el usuario y delegar la instanciacion del trabajo (Job).
    ///
    /// [ENTRADAS] Consume la `ruta_pendiente` e `indice_formato` del estado interno.
    ///
    /// [FLUJO DE EJECUCIÓN] 
    /// |> Extrae la ruta pendiente consumiendola.
    /// |> Resuelve el tipo de formato basado en el indice seleccionado.
    /// |> Muta el estado interno restaurando los parametros de seleccion de formato.
    /// |> Delega a `crear_trabajo` la construccion con los parametros recolectados.
    ///
    /// [RESULTADO] `Result<(), String>`
    ///
    /// [EXCEPCIONES] Retorna `Err` si no existe una ruta pendiente en el estado.
    pub fn confirmar_formato(&mut self) -> Result<(), String> {
        let ruta = self.ruta_pendiente.take().ok_or("Sin ruta pendiente")?;
        let formato = OutputFormat::OPCIONES[self.indice_formato];
        self.seleccionando_formato = false;
        self.indice_formato = 0;
        self.crear_trabajo(ruta, formato)
    }

    /// [COMPONENTE] Controlador de creacion de trabajos (Job).
    ///
    /// [PROPÓSITO] Construir, persistir y encolar la entidad Job en base a los parametros confirmados,
    /// aplicando validaciones de proteccion frente a la inmadurez de infraestructura (motores cargando).
    ///
    /// [ENTRADAS] 
    /// - `ruta`: Ruta del archivo objetivo almacenada como String.
    /// - `formato`: El formato de salida destino.
    ///
    /// [FLUJO DE EJECUCIÓN] 
    /// |> Valida el bloqueo de control contra inicializaciones asincronas del motor.
    /// |> Ejecuta alertas preventivas en caso de usar el motor degradado (Stub).
    /// |> Valida mediante iteracion si el archivo base ya se encuentra en cola para prevenir repeticion de carga.
    /// |> Delega a `analizador_documentos` la traduccion del archivo fisico hacia el modelo interno (Document).
    /// |> Muta el Repositorio inyectado salvando el estado Queue de la nueva estructura.
    /// |> Asigna el Job la lista viva, genera logs y delega el arranque background.
    ///
    /// [RESULTADO] `Result<(), String>`
    ///
    /// [EXCEPCIONES] Retorna `Err` si el archivo ya esta encolado, si el Engine esta bloqueado o problemas parseando IO del Document.
    fn crear_trabajo(&mut self, ruta: String, formato: OutputFormat) -> Result<(), String> {
        if !self.motor_cargado {
            return Err(
                "Motor OCR aun inicializando. Espere a que finalice la carga y vuelva a intentarlo."
                    .to_string(),
            );
        }

        if self.motor_fallido {
            self.loguear(
                "ADVERTENCIA: Motor ONNX no disponible. Los resultados seran ficticios (stub)."
                    .to_string(),
            );
        }

        let ruta_path = std::path::Path::new(&ruta);
        let ya_existe = self.trabajos.iter().any(|j| {
            j.document.source_path == ruta_path
                && matches!(j.status, JobStatus::Queued | JobStatus::Processing)
        });
        if ya_existe {
            return Err(format!("El archivo ya esta en cola: {}", ruta));
        }

        let documento = self
            .analizador_documentos
            .parse(std::path::Path::new(&ruta))
            .map_err(|e| format!("Error al parsear: {}", e))?;

        let id_trabajo = uuid::Uuid::new_v4().to_string();
        let trabajo = Job {
            id: id_trabajo.clone(),
            document: documento,
            status: JobStatus::Queued,
            created_at: std::time::SystemTime::now(),
            completed_at: None,
            profile: self.perfil,
            error_message: None,
            formato_salida: formato,
        };

        if let Err(e) = self.job_store.save(&trabajo) {
            log::warn!("No se pudo persistir el job {}: {}", id_trabajo, e);
        }

        self.trabajos.push(trabajo);
        self.indice_seleccionado = self.trabajos.len() - 1;

        let mensaje_log = format!("Nuevo trabajo creado: {} ({})", &id_trabajo[..8], formato.nombre());
        self.loguear(mensaje_log);
        self.iniciar_procesamiento_fondo(id_trabajo);

        Ok(())
    }

    /// [COMPONENTE] Orquestador de hilos de ejecucion (Spawner).
    ///
    /// [PROPÓSITO] Separar y aislar la computacion costosa (OCR) del bloque sincronizador (TUI), promoviendo 
    /// un diseno verdaderamente concurrente basado en pasos de mensajes puros (mpsc).
    ///
    /// [ENTRADAS] 
    /// - `id_trabajo`: Cadena del identificador unico a procesar.
    ///
    /// [FLUJO DE EJECUCIÓN] 
    /// |> Busca el indice estructural del Job local asociado.
    /// |> Muta el indicador de progreso y el estado a Procesando.
    /// |> Inicializa el canal asincrono (TX/RX) y anexa el RX al manejador global continuo.
    /// |> Crea el flag de detencion atomico y lo almacena habilitando la cancelacion iterativa.
    /// |> Delega dentro de un Thread OS el ciclo de vida del Job configurando pre-/post-process.
    /// |> Transmite por puente el resultado final sin bloquear la TUI.
    ///
    /// [RESULTADO] Ninguno / Side Effects asincronos.
    ///
    /// [EXCEPCIONES] Retorna vacio silenciosamente si la ID ha desaparecido de la lista central.
    fn iniciar_procesamiento_fondo(&mut self, id_trabajo: String) {
        let posicion = self.trabajos.iter().position(|j| j.id == id_trabajo);
        let indice = match posicion {
            Some(i) => i,
            None => return,
        };

        self.trabajos[indice].status = JobStatus::Processing;
        self.fase_actual.insert(id_trabajo.clone(), "Iniciando...".to_string());
        self.progreso_actual.insert(id_trabajo.clone(), 0.0);

        let (tx, rx) = std::sync::mpsc::channel();
        self.receptores_activos.insert(id_trabajo.clone(), rx);

        // Crear flag de cancelacion para este job
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.cancelaciones_activas.insert(id_trabajo.clone(), Arc::clone(&cancel_flag));

        let analizador = Arc::clone(&self.analizador_documentos);
        let motor = Arc::clone(&self.motor_ocr);
        let perfil = self.trabajos[indice].profile;
        let ruta = self.trabajos[indice].document.source_path.clone();
        let idioma = self.idioma.clone();

        std::thread::spawn(move || {
            let postprocesador = TextPostprocessor::new().with_language(&idioma.primary);
            let mut pipeline = OcrPipeline::new(analizador, Arc::clone(&motor))
                .with_postprocessor(Arc::new(postprocesador));

            if !motor.provides_layout() {
                use crate::infrastructure::layout_engines::XyCutLayoutEngine;
                log::info!("Usando XyCutLayoutEngine como motor de layout");
                pipeline = pipeline.with_layout_engine(Arc::new(XyCutLayoutEngine::new()));
            }

            match pipeline.procesar_documento(&ruta, &perfil, Some(&tx), Some(&cancel_flag)) {
                Ok(documento) => {
                    let _ = tx.send(PipelineEvent::Completado(documento));
                }
                Err(e) => {
                    let _ = tx.send(PipelineEvent::Error(e.to_string()));
                }
            }
        });

        log::info!("Procesamiento iniciado para trabajo {}", self.trabajos[indice].id);
    }

    /// Cancela el trabajo actualmente seleccionado si esta en progreso.
    pub fn cancelar_trabajo_seleccionado(&mut self) {
        let id_trabajo = match self.trabajos.get(self.indice_seleccionado) {
            Some(j) if j.status == JobStatus::Processing => j.id.clone(),
            _ => return,
        };

        if let Some(flag) = self.cancelaciones_activas.get(&id_trabajo) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            self.loguear(format!("Cancelacion solicitada: {}", &id_trabajo[..8]));
            self.mostrar_estado(format!("Cancelando job {}...", &id_trabajo[..8]));
        }
    }

    /// Arranca el thread de inicializacion del motor ONNX en background.
    ///
    /// Descarga y sesion ONNX pueden tardar minutos; correrlos en background
    /// mantiene la TUI responsiva durante la pantalla de carga inicial.
    pub fn iniciar_carga_motor(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<MotorCargaEvento>();
        self.receptor_motor = Some(rx);
        self.progreso_carga_motor = 0.0;
        self.fase_carga_motor = "Verificando modelos...".to_string();
        self.loguear("Iniciando descarga/carga de modelos ONNX...".to_string());

        std::thread::spawn(move || {
            use crate::infrastructure::ocr_engines::onnx::{gpu_config, ModelDownloader, OnnxOcrEngine};

            let estado_gpu = gpu_config::inicializar(0);
            let _ = tx.send(MotorCargaEvento::GpuInfo {
                backend: estado_gpu.backend_compilado.to_string(),
                activo: estado_gpu.inicializado && estado_gpu.es_gpu,
            });

            let downloader = match ModelDownloader::new() {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx.send(MotorCargaEvento::Error(e.to_string()));
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
                let _ = tx_bytes.send(MotorCargaEvento::DescargandoBytes { bytes_actual, bytes_total });
            };

            let ruta_modelos = match downloader.asegurar_todos_los_modelos(Some(&on_archivo), Some(&on_bytes)) {
                Ok(ruta) => ruta,
                Err(e) => {
                    let _ = tx.send(MotorCargaEvento::Error(e.to_string()));
                    return;
                }
            };

            match OnnxOcrEngine::from_directory(&ruta_modelos) {
                Ok(engine) => {
                    let _ = tx.send(MotorCargaEvento::Listo(Arc::new(engine)));
                }
                Err(e) => {
                    let _ = tx.send(MotorCargaEvento::Error(e.to_string()));
                }
            }
        });
    }

    /// Drena todos los canales activos en un tick del event loop.
    ///
    /// Motor primero: garantiza que el swap ONNX->stub ocurra antes de procesar
    /// eventos de pipelines que se pudieron lanzar justo despues del swap.
    pub fn consultar_trabajos(&mut self) {
        self.consultar_motor();
        self.consultar_pipelines();
    }

    /// Procesa eventos del canal de carga del motor.
    fn consultar_motor(&mut self) {
        let evento = match &self.receptor_motor {
            Some(rx) => match rx.try_recv() {
                Ok(e) => e,
                Err(_) => return,
            },
            None => return,
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
                self.loguear(format!("Aceleracion: {}", self.gpu_info));
            }
            MotorCargaEvento::Descargando { nombre, actual, total } => {
                self.progreso_carga_motor = actual as f32 / total as f32;
                self.fase_carga_motor = format!("Descargando {} ({}/{})", nombre, actual, total);
                self.bytes_carga_actual = 0;
                self.bytes_carga_total = 0;
                self.loguear(format!("Descargando modelo {}/{}: {}", actual, total, nombre));
            }
            MotorCargaEvento::DescargandoBytes { bytes_actual, bytes_total } => {
                self.bytes_carga_actual = bytes_actual;
                self.bytes_carga_total = bytes_total;
            }
            MotorCargaEvento::Listo(motor) => {
                self.motor_ocr = motor;
                self.motor_cargado = true;
                self.progreso_carga_motor = 1.0;
                if self.vista_actual == ViewMode::Initializing {
                    self.vista_actual = ViewMode::JobList;
                }
                self.loguear("Motor OCR ONNX cargado exitosamente".to_string());
                self.mostrar_estado("Motor OCR listo".to_string());
                self.receptor_motor = None;
            }
            MotorCargaEvento::Error(e) => {
                self.motor_cargado = true;
                self.motor_fallido = true;
                if self.vista_actual == ViewMode::Initializing {
                    self.vista_actual = ViewMode::JobList;
                }
                self.loguear(format!("Error motor ONNX: {}. Usando Stub (resultados ficticios).", e));
                self.mostrar_estado(format!("Error motor ONNX: {}", e));
                self.receptor_motor = None;
            }
        }
    }

    /// Drena los canales de todos los pipelines activos en un tick.
    ///
    /// Recoleccion y procesamiento de eventos estan separados para evitar borrow
    /// conflicts al mutar `self` mientras se itera `receptores_activos`.
    fn consultar_pipelines(&mut self) {
        let mut trabajos_terminados: Vec<String> = Vec::new();
        let ids_trabajos: Vec<String> = self.receptores_activos.keys().cloned().collect();

        for id_trabajo in &ids_trabajos {
            let eventos = self.recolectar_eventos(id_trabajo);

            for evento in eventos {
                match evento {
                    PipelineEvent::FaseCambiada { fase, progreso } => {
                        self.fase_actual.insert(id_trabajo.clone(), fase);
                        self.progreso_actual.insert(id_trabajo.clone(), progreso);
                    }
                    PipelineEvent::ProgresoActualizado { pagina_actual, total_paginas } => {
                        if let Some(fase) = self.fase_actual.get_mut(id_trabajo) {
                            *fase = format!(
                                "{} (pagina {}/{})",
                                fase, pagina_actual, total_paginas
                            );
                        }
                    }
                    PipelineEvent::Completado(documento) => {
                        let mut ruta_export: Option<std::path::PathBuf> = None;
                        let mut export_resultado: Option<Result<(), String>> = None;

                        if let Some(trabajo) = self.trabajos.iter_mut().find(|j| &j.id == id_trabajo) {
                            trabajo.document = documento;
                            trabajo.status = JobStatus::Completed;
                            trabajo.completed_at = Some(std::time::SystemTime::now());

                            let ruta = trabajo.document.source_path
                                .with_extension(trabajo.formato_salida.extension());
                            let resultado = exportar_segun_formato(trabajo, &ruta)
                                .map_err(|e| e.to_string());
                            ruta_export = Some(ruta);
                            export_resultado = Some(resultado);

                            if let Err(e) = self.job_store.update(trabajo) {
                                log::warn!("No se pudo actualizar job {} en disco: {}", id_trabajo, e);
                            }
                        }

                        match export_resultado {
                            Some(Ok(_)) => {
                                if let Some(ref ruta) = ruta_export {
                                    self.loguear(format!("Auto-export: {}", ruta.display()));
                                }
                            }
                            Some(Err(e)) => {
                                self.loguear(format!("Error auto-export {}: {}", &id_trabajo[..8], e));
                            }
                            None => {}
                        }

                        self.loguear(format!("Trabajo {} completado", &id_trabajo[..8]));
                        self.mostrar_estado(format!("Trabajo {} completado", &id_trabajo[..8]));
                        trabajos_terminados.push(id_trabajo.clone());
                    }
                    PipelineEvent::Error(mensaje) => {
                        // Cancelacion y error real comparten el mismo variante para simplificar
                        // el pipeline (evita un PipelineEvent::Cancelado adicional). La distincion
                        // se hace aqui por string porque es la unica capa que conoce MSG_JOB_CANCELADO
                        // y necesita mapear a JobStatus::Cancelled vs JobStatus::Failed.
                        let es_cancelacion = mensaje == MSG_JOB_CANCELADO;

                        if let Some(trabajo) = self.trabajos.iter_mut().find(|j| &j.id == id_trabajo) {
                            if es_cancelacion {
                                trabajo.status = JobStatus::Cancelled;
                                trabajo.error_message = None;
                            } else {
                                trabajo.status = JobStatus::Failed;
                                trabajo.error_message = Some(mensaje.clone());
                            }

                            if let Err(e) = self.job_store.update(trabajo) {
                                log::warn!("No se pudo actualizar job {} en disco: {}", id_trabajo, e);
                            }

                            if es_cancelacion {
                                self.loguear(format!("Trabajo {} cancelado", &id_trabajo[..8]));
                                self.mostrar_estado(format!("Job {} cancelado", &id_trabajo[..8]));
                            } else {
                                self.loguear(format!("Error en trabajo {}: {}", &id_trabajo[..8], mensaje));
                                self.mostrar_estado(format!("Error en {}: {}", &id_trabajo[..8], mensaje));
                            }
                        }
                        trabajos_terminados.push(id_trabajo.clone());
                    }
                }
            }
        }

        for id_trabajo in &trabajos_terminados {
            self.receptores_activos.remove(id_trabajo);
            self.fase_actual.remove(id_trabajo);
            self.progreso_actual.remove(id_trabajo);
            self.cancelaciones_activas.remove(id_trabajo);
        }
    }

    /// Drena todos los eventos disponibles de un canal sin bloquearse.
    ///
    /// Acumular todos los eventos del tick en un `Vec` antes de procesarlos
    /// permite que `consultar_pipelines` mute `self` libremente durante el procesamiento
    /// sin mantener un borrow activo sobre `receptores_activos`. El coste de la
    /// coalescencia es minimo: en condiciones normales hay 1-2 eventos por tick.
    fn recolectar_eventos(&self, id_trabajo: &str) -> Vec<PipelineEvent> {
        let mut eventos = Vec::new();

        if let Some(rx) = self.receptores_activos.get(id_trabajo) {
            loop {
                match rx.try_recv() {
                    Ok(evento) => eventos.push(evento),
                    Err(_) => break,
                }
            }
        }

        eventos
    }

    pub fn hay_trabajos_en_progreso(&self) -> bool {
        !self.receptores_activos.is_empty()
    }

    pub fn cambiar_vista(&mut self, vista: ViewMode) {
        self.vista_actual = vista;
        self.scroll_detalle = 0;
    }

    pub fn salir(&mut self) {
        self.debe_salir = true;
    }

    pub fn eliminar_trabajo_seleccionado(&mut self) {
        if self.trabajos.is_empty() {
            return;
        }

        let id_trabajo = self.trabajos[self.indice_seleccionado].id.clone();
        self.trabajos.remove(self.indice_seleccionado);

        self.receptores_activos.remove(&id_trabajo);
        self.fase_actual.remove(&id_trabajo);
        self.progreso_actual.remove(&id_trabajo);
        self.cancelaciones_activas.remove(&id_trabajo);

        if let Err(e) = self.job_store.delete(&id_trabajo) {
            log::warn!("No se pudo eliminar job {} de disco: {}", id_trabajo, e);
        }

        if self.indice_seleccionado >= self.trabajos.len() && !self.trabajos.is_empty() {
            self.indice_seleccionado = self.trabajos.len() - 1;
        }

        self.loguear(format!("Trabajo {} eliminado", &id_trabajo[..8]));
    }

    pub fn exportar_trabajo_markdown(&mut self) {
        let trabajo = match self.obtener_trabajo_seleccionado() {
            Some(j) => j.clone(),
            None => return,
        };

        let ruta_base = trabajo.document.source_path.with_extension("md");
        let exportador = MarkdownExporter::new();

        match exportador.export(&trabajo, &ruta_base) {
            Ok(_) => {
                self.loguear(format!("Exportado MD: {}", ruta_base.display()));
                self.mostrar_estado(format!("Exportado: {}", ruta_base.display()));
            }
            Err(e) => {
                self.loguear(format!("Error exportacion MD: {}", e));
                self.mostrar_estado(format!("Error exportacion: {}", e));
            }
        }
    }

    pub fn exportar_trabajo_json(&mut self) {
        let trabajo = match self.obtener_trabajo_seleccionado() {
            Some(j) => j.clone(),
            None => return,
        };

        let ruta_base = trabajo.document.source_path.with_extension("json");
        let exportador = JsonExporter::new();

        match exportador.export(&trabajo, &ruta_base) {
            Ok(_) => {
                self.loguear(format!("Exportado JSON: {}", ruta_base.display()));
                self.mostrar_estado(format!("Exportado: {}", ruta_base.display()));
            }
            Err(e) => {
                self.loguear(format!("Error exportacion JSON: {}", e));
                self.mostrar_estado(format!("Error exportacion: {}", e));
            }
        }
    }

    pub fn exportar_trabajo_pdf(&mut self) {
        let trabajo = match self.obtener_trabajo_seleccionado() {
            Some(j) => j.clone(),
            None => return,
        };

        let ruta_base = trabajo.document.source_path.with_extension("pdf");
        let exportador = PdfSandwichExporter::new();

        match exportador.export(&trabajo, &ruta_base) {
            Ok(_) => {
                self.loguear(format!("Exportado PDF: {}", ruta_base.display()));
                self.mostrar_estado(format!("Exportado: {}", ruta_base.display()));
            }
            Err(e) => {
                self.loguear(format!("Error exportacion PDF: {}", e));
                self.mostrar_estado(format!("Error exportacion: {}", e));
            }
        }
    }

    pub fn mostrar_estado(&mut self, mensaje: String) {
        self.mensaje_estado = Some((mensaje, std::time::Instant::now()));
    }

    pub fn obtener_estado(&self) -> Option<&str> {
        self.mensaje_estado.as_ref().and_then(|(msg, time)| {
            if time.elapsed() < std::time::Duration::from_secs(3) {
                Some(msg.as_str())
            } else {
                None
            }
        })
    }

    pub fn limpiar_trabajos_finalizados(&mut self) {
        let antes = self.trabajos.len();

        let ids_a_eliminar: Vec<String> = self.trabajos.iter()
            .filter(|j| {
                j.status == JobStatus::Completed
                    || j.status == JobStatus::Failed
                    || j.status == JobStatus::Cancelled
            })
            .map(|j| j.id.clone())
            .collect();

        for id in &ids_a_eliminar {
            if let Err(e) = self.job_store.delete(id) {
                log::warn!("No se pudo eliminar job {} de disco al limpiar: {}", id, e);
            }
        }

        self.trabajos.retain(|j| {
            j.status != JobStatus::Completed
                && j.status != JobStatus::Failed
                && j.status != JobStatus::Cancelled
        });
        let despues = self.trabajos.len();

        let ids_activos: Vec<String> = self.trabajos.iter().map(|j| j.id.clone()).collect();
        self.fase_actual.retain(|k, _| ids_activos.contains(k));
        self.progreso_actual.retain(|k, _| ids_activos.contains(k));
        self.receptores_activos.retain(|k, _| ids_activos.contains(k));
        self.cancelaciones_activas.retain(|k, _| ids_activos.contains(k));

        if self.indice_seleccionado >= self.trabajos.len() && !self.trabajos.is_empty() {
            self.indice_seleccionado = self.trabajos.len() - 1;
        } else if self.trabajos.is_empty() {
            self.indice_seleccionado = 0;
        }

        if antes > despues {
            self.loguear(format!("Se limpiaron {} trabajos", antes - despues));
        } else {
            self.loguear("No hay trabajos finalizados para limpiar".to_string());
        }
    }

    pub fn scroll_detalle_abajo(&mut self) {
        self.scroll_detalle = self.scroll_detalle.saturating_add(1);
    }

    pub fn scroll_detalle_arriba(&mut self) {
        self.scroll_detalle = self.scroll_detalle.saturating_sub(1);
    }
}

/// Exporta un job al formato solicitado en la ruta indicada.
fn exportar_segun_formato(job: &Job, ruta: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    match job.formato_salida {
        OutputFormat::Markdown => MarkdownExporter::new().export(job, ruta).map_err(|e| e.into()),
        OutputFormat::Pdf      => PdfSandwichExporter::new().export(job, ruta).map_err(|e| e.into()),
        OutputFormat::Json     => JsonExporter::new().export(job, ruta).map_err(|e| e.into()),
    }
}

/// Carga los trabajos previos desde el store al arranque.
/// Retorna la lista de trabajos y mensajes de log para mostrar en la TUI.
fn cargar_trabajos_iniciales(store: &dyn JobStorePort) -> (Vec<Job>, Vec<String>) {
    let mut mensajes = Vec::new();

    match store.list() {
        Ok(mut jobs) => {
            let total = jobs.len();
            normalizar_jobs_al_arranque(&mut jobs);

            let interrumpidos = jobs.iter()
                .filter(|j| j.error_message.as_deref() == Some(
                    "Interrumpido: la aplicacion se cerro durante el procesamiento"
                ))
                .count();

            // Persistir los jobs normalizados (Processing -> Failed)
            for job in &jobs {
                if job.status == JobStatus::Failed
                    && job.error_message.as_deref() == Some(
                        "Interrumpido: la aplicacion se cerro durante el procesamiento"
                    )
                {
                    let _ = store.update(job);
                }
            }

            if total > 0 {
                mensajes.push(format!("{} trabajos recuperados de sesiones anteriores", total));
            }
            if interrumpidos > 0 {
                mensajes.push(format!(
                    "{} trabajo(s) marcados como fallidos (interrumpidos)",
                    interrumpidos
                ));
            }

            (jobs, mensajes)
        }
        Err(e) => {
            log::warn!("No se pudieron cargar jobs previos: {}", e);
            mensajes.push("No se pudieron cargar trabajos anteriores".to_string());
            (Vec::new(), mensajes)
        }
    }
}
