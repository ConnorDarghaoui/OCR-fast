/// Cadena opcional de refinamientos encadenables por etapa del pipeline.
pub mod refinement;

use crate::domain::{BlockType, Document, DocumentBlueprint, ProcessingProfile};
use crate::infrastructure::page_composer::PageComposer;
use crate::interfaces::ports::{
    DocumentAssemblerPort, DocumentBlueprintBuilderPort, DocumentParserPort, LayoutEnginePort,
    OcrEnginePort, PostprocessorPort, PreprocessorPort, TableAnalyzerPort,
};
pub use refinement::{
    ConfidenceBoostPass, DenoisePass, DeskewPass, NoopRefinementPass, RefinementBudget,
    RefinementContext, RefinementPass, RefinementStage,
};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use thiserror::Error;

/// Fases operativas del pipeline usadas para tipar fallos terminales.
///
/// Mantener la fase como enum evita depender de mensajes libres para auditoría,
/// persistencia o decisiones de reintento en capas superiores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Parseo,
    Preprocesamiento,
    Layout,
    Ocr,
    Tablas,
    Postproceso,
    Ensamblado,
    Blueprint,
    Refinamiento,
}

impl std::fmt::Display for PipelineStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nombre = match self {
            Self::Parseo => "parseo",
            Self::Preprocesamiento => "preprocesamiento",
            Self::Layout => "layout",
            Self::Ocr => "ocr",
            Self::Tablas => "tablas",
            Self::Postproceso => "postproceso",
            Self::Ensamblado => "ensamblado",
            Self::Blueprint => "blueprint",
            Self::Refinamiento => "refinamiento",
        };

        f.write_str(nombre)
    }
}

/// Error terminal tipado del pipeline OCR.
///
/// `Cancelado` modela una salida cooperativa esperable, mientras `Fase` conserva
/// la etapa de fallo para mejorar trazabilidad y decisiones de recuperación.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PipelineFailure {
    /// Cancelación solicitada por el usuario y observada en una frontera segura.
    #[error("Job cancelado por el usuario")]
    Cancelado,

    /// Fallo terminal producido por una fase concreta del pipeline.
    #[error("error en fase {fase}: {mensaje}")]
    Fase {
        fase: PipelineStage,
        mensaje: String,
    },
}

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

    Error(PipelineFailure),
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
    refinement_passes: Vec<Arc<dyn RefinementPass>>,
    refinement_budget: RefinementBudget,
    ensamblador_documento: Option<Arc<dyn DocumentAssemblerPort>>,
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
            refinement_passes: Vec::new(),
            refinement_budget: RefinementBudget::default(),
            ensamblador_documento: None,
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
    ///
    /// Este builder se mantiene como override legacy. La ruta canónica para
    /// `procesar_documento_con_blueprint()` ya es `PageComposer`; por eso este
    /// puerto solo debería existir mientras haya callers históricos o pruebas
    /// que necesiten validar la compatibilidad.
    pub fn with_blueprint_builder(
        mut self,
        blueprint_builder: Arc<dyn DocumentBlueprintBuilderPort>,
    ) -> Self {
        self.blueprint_builder = Some(blueprint_builder);
        self
    }

    /// Añade un pass de refinamiento opcional en la etapa declarada por el pass.
    pub fn with_refinement_pass(mut self, refinement_pass: Arc<dyn RefinementPass>) -> Self {
        self.refinement_passes.push(refinement_pass);
        self
    }

    /// Configura el presupuesto máximo de passes por corrida del pipeline.
    pub fn with_refinement_budget(mut self, refinement_budget: RefinementBudget) -> Self {
        self.refinement_budget = refinement_budget;
        self
    }

    /// Añade una fase final que reconstruye el documento guiándose por layout.
    pub fn with_document_assembler(
        mut self,
        ensamblador_documento: Arc<dyn DocumentAssemblerPort>,
    ) -> Self {
        self.ensamblador_documento = Some(ensamblador_documento);
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
    /// o postproceso como `PipelineFailure::Fase`. La cancelación se materializa
    /// como `PipelineFailure::Cancelado`.
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
    ) -> Result<Document, PipelineFailure> {
        self.procesar_documento_interno(ruta, perfil, notificador, cancelacion, false)
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
    ) -> Result<PipelineResult, PipelineFailure> {
        self.procesar_documento_interno(ruta, perfil, notificador, cancelacion, true)
    }

    fn procesar_documento_interno(
        &self,
        ruta: &Path,
        perfil: &ProcessingProfile,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
        construir_blueprint: bool,
    ) -> Result<PipelineResult, PipelineFailure> {
        let mut refinement_consumidos = 0usize;
        self.notificar(
            notificador,
            PipelineEvent::FaseCambiada {
                fase: "Parseando documento".to_string(),
                progreso: 0.0,
            },
        );

        let mut documento = self
            .parser
            .parse(ruta)
            .map_err(|error| Self::error_fase(PipelineStage::Parseo, error))?;
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
            preprocesador
                .preprocess(&mut documento)
                .map_err(|error| Self::error_fase(PipelineStage::Preprocesamiento, error))?;
            log::info!("Pipeline: preprocesamiento completado");
        }

        self.verificar_cancelacion(cancelacion)?;
        let mut blueprint = None;
        self.ejecutar_refinamientos(
            RefinementStage::BeforeLayout,
            &mut documento,
            &mut blueprint,
            ruta,
            perfil,
            notificador,
            cancelacion,
            &mut refinement_consumidos,
        )?;

        if let Some(ref layout_engine) = self.layout_engine {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Analizando layout".to_string(),
                    progreso: 0.30,
                },
            );
            for (i, pagina) in documento.pages.iter_mut().enumerate() {
                let bloques = layout_engine
                    .analyze(pagina)
                    .map_err(|error| Self::error_fase(PipelineStage::Layout, error))?;
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
        self.ocr_engine
            .process(&mut documento, perfil)
            .map_err(|error| Self::error_fase(PipelineStage::Ocr, error))?;
        log::info!("Pipeline: OCR completado ({})", self.ocr_engine.name());

        self.verificar_cancelacion(cancelacion)?;
        self.ejecutar_refinamientos(
            RefinementStage::AfterOcr,
            &mut documento,
            &mut blueprint,
            ruta,
            perfil,
            notificador,
            cancelacion,
            &mut refinement_consumidos,
        )?;

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
                table_analyzer
                    .analyze_tables(&mut documento)
                    .map_err(|error| Self::error_fase(PipelineStage::Tablas, error))?;
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
            postprocesador
                .postprocess(&mut documento)
                .map_err(|error| Self::error_fase(PipelineStage::Postproceso, error))?;
            log::info!("Pipeline: postprocesamiento completado");
        }

        self.verificar_cancelacion(cancelacion)?;
        self.ejecutar_refinamientos(
            RefinementStage::BeforeBlueprint,
            &mut documento,
            &mut blueprint,
            ruta,
            perfil,
            notificador,
            cancelacion,
            &mut refinement_consumidos,
        )?;

        if let Some(ref ensamblador_documento) = self.ensamblador_documento {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Reconstruyendo documento final".to_string(),
                    progreso: 0.95,
                },
            );
            ensamblador_documento
                .assemble(&mut documento)
                .map_err(|error| Self::error_fase(PipelineStage::Ensamblado, error))?;
            log::info!(
                "Pipeline: ensamblado final completado ({})",
                ensamblador_documento.name()
            );
        }

        self.verificar_cancelacion(cancelacion)?;

        let mut blueprint = if construir_blueprint {
            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: "Reconstruyendo blueprint visual".to_string(),
                    progreso: 0.97,
                },
            );
            Some(self.construir_blueprint_canonico(&documento)?)
        } else {
            None
        };

        self.ejecutar_refinamientos(
            RefinementStage::AfterBlueprint,
            &mut documento,
            &mut blueprint,
            ruta,
            perfil,
            notificador,
            cancelacion,
            &mut refinement_consumidos,
        )?;

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

    fn construir_blueprint_canonico(
        &self,
        documento: &Document,
    ) -> Result<DocumentBlueprint, PipelineFailure> {
        if let Some(ref blueprint_builder) = self.blueprint_builder {
            let blueprint = blueprint_builder
                .build_blueprint(documento)
                .map_err(|error| Self::error_fase(PipelineStage::Blueprint, error))?;
            log::info!(
                "Pipeline: blueprint visual completado ({}) [compat]",
                blueprint_builder.name()
            );
            Ok(blueprint)
        } else {
            let blueprint = PageComposer::new()
                .compose(documento)
                .map_err(|error| Self::error_fase(PipelineStage::Blueprint, error))?;
            log::info!("Pipeline: blueprint visual completado (PageComposer)");
            Ok(blueprint)
        }
    }

    fn verificar_cancelacion(
        &self,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<(), PipelineFailure> {
        if cancelacion.map_or(false, |f| f.load(Ordering::Relaxed)) {
            return Err(PipelineFailure::Cancelado);
        }
        Ok(())
    }

    fn error_fase(
        fase: PipelineStage,
        error: impl std::error::Error + Send + Sync + 'static,
    ) -> PipelineFailure {
        PipelineFailure::Fase {
            fase,
            mensaje: error.to_string(),
        }
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

    fn ejecutar_refinamientos(
        &self,
        stage: RefinementStage,
        documento: &mut Document,
        blueprint: &mut Option<DocumentBlueprint>,
        ruta: &Path,
        perfil: &ProcessingProfile,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
        refinement_consumidos: &mut usize,
    ) -> Result<(), PipelineFailure> {
        if self.refinement_passes.is_empty() {
            return Ok(());
        }

        let total_paginas = documento.pages.len() as u32;
        for refinement_pass in self
            .refinement_passes
            .iter()
            .filter(|pass| pass.stage() == stage)
        {
            self.verificar_cancelacion(cancelacion)?;
            if !self.refinement_budget.allows(*refinement_consumidos) {
                log::info!(
                    "Pipeline: presupuesto de refinamiento agotado antes de {}",
                    refinement_pass.name()
                );
                break;
            }

            self.notificar(
                notificador,
                PipelineEvent::FaseCambiada {
                    fase: format!("Refinando documento ({})", refinement_pass.name()),
                    progreso: progreso_refinamiento(stage),
                },
            );

            let contexto = RefinementContext {
                source_path: ruta,
                profile: perfil,
                total_pages: total_paginas,
                stage,
                consumed_passes: *refinement_consumidos,
                remaining_passes: self.refinement_budget.remaining(*refinement_consumidos),
            };

            refinement_pass
                .refine(documento, blueprint, &contexto)
                .map_err(|error| Self::error_refinamiento(stage, refinement_pass.name(), error))?;
            *refinement_consumidos += 1;
            log::info!(
                "Pipeline: refinamiento completado ({})",
                refinement_pass.name()
            );
        }

        Ok(())
    }

    fn error_refinamiento(
        _stage: RefinementStage,
        nombre_pass: &str,
        error: Box<dyn std::error::Error + Send + Sync>,
    ) -> PipelineFailure {
        PipelineFailure::Fase {
            fase: PipelineStage::Refinamiento,
            mensaje: format!("refinamiento {nombre_pass}: {error}"),
        }
    }
}

fn progreso_refinamiento(stage: RefinementStage) -> f32 {
    match stage {
        RefinementStage::BeforeLayout => 0.22,
        RefinementStage::AfterOcr => 0.62,
        RefinementStage::BeforeBlueprint => 0.93,
        RefinementStage::AfterBlueprint => 0.985,
    }
}
