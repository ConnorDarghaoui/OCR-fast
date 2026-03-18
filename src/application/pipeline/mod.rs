use crate::domain::{BlockType, Document, DocumentBlueprint, ProcessingProfile};
use crate::interfaces::ports::{
    DocumentBlueprintBuilderPort, DocumentParserPort, LayoutEnginePort, OcrEnginePort,
    PostprocessorPort, PreprocessorPort, TableAnalyzerPort,
};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Mensaje canónico usado para mapear cancelación cooperativa a estado de UI.
///
/// Se mantiene como constante pública porque pipeline y TUI viven en módulos
/// distintos y ambos necesitan una convención estable sin introducir un tipo de
/// error adicional solo para cancelación. Es una decisión pragmática: reduce
/// complejidad del canal a costa de depender de una cadena sentinela.
pub const MSG_JOB_CANCELADO: &str = "Job cancelado por el usuario";

/// Eventos emitidos por el pipeline hacia la TUI mediante `mpsc`.
///
/// La separación entre eventos de fase, progreso y resultado final evita que la
/// UI tenga que inferir semántica temporal a partir de un payload ambiguo. El
/// enum está pensado para transporte entre hilos, no como dominio persistible.
///
/// # Concurrency
///
/// Las variantes poseen ownership completo para cruzar canales sin lifetimes ni
/// referencias prestadas, lo que simplifica cancelación, buffering y testing.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// Transicion a una nueva fase con su progreso global estimado en [0.0, 1.0].
    FaseCambiada {
        fase: String,
        progreso: f32,
    },

    /// Avance de pagina dentro de la fase activa.
    ProgresoActualizado {
        pagina_actual: u32,
        total_paginas: u32,
    },

    /// Documento procesado listo para exportar. Se envia por canal porque el
    /// pipeline corre en un thread separado sin acceso directo a `AppState`.
    Completado(Document),

    Error(String),
}

/// Resultado enriquecido del pipeline OCR completo.
///
/// El retorno separa el `Document` operativo del blueprint visual opcional para
/// no forzar a todos los callers a pagar ni depender de exportación de alta
/// fidelidad. La estructura mantiene compatibilidad progresiva con la API previa
/// basada solo en `Document`.
pub struct PipelineResult {
    /// Documento OCR con bloques y contenido listo para persistencia o UI.
    pub document: Document,
    /// Modelo intermedio opcional para exportadores visuales ricos.
    pub blueprint: Option<DocumentBlueprint>,
}

/// Orquestador inmutable del pipeline OCR multi-fase.
///
/// `OcrPipeline` encapsula las dependencias por etapa y usa un builder ligero
/// para habilitar u omitir fases sin introducir una jerarquía compleja de tipos.
/// La estructura es inmutable tras su construcción para que pueda ejecutarse en
/// background sin carreras sobre configuración compartida.
///
/// # Trade-offs
///
/// El builder sacrifica validación de composición en compile-time a cambio de una
/// API simple para la TUI. Dado el número acotado de fases opcionales, el costo
/// de esa flexibilidad es razonable.
pub struct OcrPipeline {
    parser: Arc<dyn DocumentParserPort>,
    preprocesador: Option<Arc<dyn PreprocessorPort>>,
    layout_engine: Option<Arc<dyn LayoutEnginePort>>,
    ocr_engine: Arc<dyn OcrEnginePort>,
    table_analyzer: Option<Arc<dyn TableAnalyzerPort>>,
    postprocesador: Option<Arc<dyn PostprocessorPort>>,
    blueprint_builder: Option<Arc<dyn DocumentBlueprintBuilderPort>>,
}

impl OcrPipeline {
    /// Crea un pipeline con las dependencias obligatorias mínimas.
    ///
    /// # Trade-offs
    ///
    /// Parser y OCR son obligatorios porque definen la ruta crítica del producto.
    /// Layout, tablas y postproceso quedan opcionales para permitir degradación
    /// controlada cuando latencia o dependencias externas importan más.
    pub fn new(parser: Arc<dyn DocumentParserPort>, ocr_engine: Arc<dyn OcrEnginePort>) -> Self {
        Self {
            parser,
            preprocesador: None,
            layout_engine: None,
            ocr_engine,
            table_analyzer: None,
            postprocesador: None,
            blueprint_builder: None,
        }
    }

    /// Añade una fase de preprocesamiento previa al análisis de layout.
    pub fn with_preprocessor(mut self, preprocesador: Arc<dyn PreprocessorPort>) -> Self {
        self.preprocesador = Some(preprocesador);
        self
    }

    /// Añade un motor externo de layout cuando el OCR no lo integra.
    pub fn with_layout_engine(mut self, layout_engine: Arc<dyn LayoutEnginePort>) -> Self {
        self.layout_engine = Some(layout_engine);
        self
    }

    /// Añade un analizador de estructura tabular posterior al OCR.
    pub fn with_table_analyzer(mut self, table_analyzer: Arc<dyn TableAnalyzerPort>) -> Self {
        self.table_analyzer = Some(table_analyzer);
        self
    }

    /// Añade una fase de postproceso textual posterior a la inferencia.
    pub fn with_postprocessor(mut self, postprocesador: Arc<dyn PostprocessorPort>) -> Self {
        self.postprocesador = Some(postprocesador);
        self
    }

    /// Añade una fase opcional de reconstrucción visual para exportadores ricos.
    pub fn with_blueprint_builder(
        mut self,
        blueprint_builder: Arc<dyn DocumentBlueprintBuilderPort>,
    ) -> Self {
        self.blueprint_builder = Some(blueprint_builder);
        self
    }

    /// Ejecuta el pipeline completo sobre un recurso de entrada.
    ///
    /// La ejecución es secuencial y cooperativa: cada fase transforma el mismo
    /// `Document` acumulado y la cancelación se verifica en fronteras estables para
    /// evitar abortos asíncronos que dejarían estado inconsistente o recursos ONNX
    /// en una condición difícil de razonar.
    ///
    /// # Errors
    ///
    /// Propaga cualquier error de parsing, preprocesamiento, layout, OCR, tablas
    /// o postproceso. La cancelación se materializa como error sentinela con el
    /// mensaje `MSG_JOB_CANCELADO`.
    ///
    /// # Concurrency
    ///
    /// El pipeline no comparte mutaciones internas entre hilos. La única frontera
    /// concurrente es el canal de notificación y el flag atómico de cancelación.
    ///
    /// # Trade-offs
    ///
    /// La cancelación entre fases es menos granular que interrumpir kernels o
    /// loops internos, pero evita introducir puntos inseguros en librerías de
    /// inferencia y mantiene la semántica de rollback bajo control.
    pub fn procesar_documento(
        &self,
        ruta: &Path,
        perfil: &ProcessingProfile,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Document, Box<dyn std::error::Error + Send + Sync>> {
        self.procesar_documento_con_blueprint(ruta, perfil, notificador, cancelacion)
            .map(|resultado| resultado.document)
    }

    /// Ejecuta el pipeline y devuelve, opcionalmente, el blueprint visual final.
    ///
    /// Esta variante conserva la misma ruta crítica del OCR tradicional pero
    /// añade una frontera explícita para reconstrucción visual posterior al
    /// postproceso. La separación permite que callers sensibles a latencia sigan
    /// consumiendo únicamente `procesar_documento`.
    pub fn procesar_documento_con_blueprint(
        &self,
        ruta: &Path,
        perfil: &ProcessingProfile,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<PipelineResult, Box<dyn std::error::Error + Send + Sync>> {
        self.notificar(
            notificador,
            PipelineEvent::FaseCambiada {
                fase: "Parseando documento".to_string(),
                progreso: 0.0,
            },
        );

        let mut documento = self.parser.parse(ruta)?;
        let total_paginas = documento.pages.len() as u32;
        log::info!("Pipeline: documento parseado ({} paginas)", total_paginas);

        self.verificar_cancelacion(cancelacion)?;

        if let Some(ref preprocesador) = self.preprocesador {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Preprocesando imagenes".to_string(),
                    progreso: 0.15,
                },
            );
            preprocesador.preprocess(&mut documento)?;
            log::info!("Pipeline: preprocesamiento completado");
        }

        self.verificar_cancelacion(cancelacion)?;

        if let Some(ref layout_engine) = self.layout_engine {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Analizando layout".to_string(),
                    progreso: 0.30,
                },
            );
            for (i, pagina) in documento.pages.iter_mut().enumerate() {
                let bloques = layout_engine.analyze(pagina)?;
                pagina.blocks = bloques;
                self.notificar(
                    notificador,
                    PipelineEvent::ProgresoActualizado {
                        pagina_actual: (i + 1) as u32,
                        total_paginas,
                    },
                );
            }
            log::info!("Pipeline: layout completado ({})", layout_engine.name());
        }

        self.verificar_cancelacion(cancelacion)?;

        self.notificar(
            notificador,
            PipelineEvent::FaseCambiada {
                fase: "Reconociendo texto (OCR)".to_string(),
                progreso: 0.50,
            },
        );
        self.ocr_engine.process(&mut documento, perfil)?;
        log::info!("Pipeline: OCR completado ({})", self.ocr_engine.name());

        self.verificar_cancelacion(cancelacion)?;

        if let Some(ref table_analyzer) = self.table_analyzer {
            let hay_tablas = documento
                .pages
                .iter()
                .any(|p| p.blocks.iter().any(|b| b.block_type == BlockType::Table));
            if hay_tablas {
                self.notificar(
                    notificador,
                    PipelineEvent::FaseCambiada {
                        fase: "Analizando tablas".to_string(),
                        progreso: 0.75,
                    },
                );
                table_analyzer.analyze_tables(&mut documento)?;
                log::info!("Pipeline: tablas completado ({})", table_analyzer.name());
            }
        }

        self.verificar_cancelacion(cancelacion)?;

        if let Some(ref postprocesador) = self.postprocesador {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Postprocesando texto".to_string(),
                    progreso: 0.90,
                },
            );
            postprocesador.postprocess(&mut documento)?;
            log::info!("Pipeline: postprocesamiento completado");
        }

        self.verificar_cancelacion(cancelacion)?;

        let blueprint = if let Some(ref blueprint_builder) = self.blueprint_builder {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Reconstruyendo blueprint visual".to_string(),
                    progreso: 0.96,
                },
            );
            let blueprint = blueprint_builder.build_blueprint(&documento)?;
            log::info!(
                "Pipeline: blueprint visual completado ({})",
                blueprint_builder.name()
            );
            Some(blueprint)
        } else {
            None
        };

        self.notificar(
            notificador,
            PipelineEvent::FaseCambiada {
                fase: "Completado".to_string(),
                progreso: 1.0,
            },
        );

        Ok(PipelineResult {
            document: documento,
            blueprint,
        })
    }

    fn verificar_cancelacion(
        &self,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if cancelacion.map_or(false, |f| f.load(Ordering::Relaxed)) {
            return Err(MSG_JOB_CANCELADO.into());
        }
        Ok(())
    }

    fn notificar(
        &self,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        evento: PipelineEvent,
    ) {
        if let Some(tx) = notificador {
            let _ = tx.send(evento);
        }
    }
}
