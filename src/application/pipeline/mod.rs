//! Orquestador del pipeline OCR de 6 fases.
//!
//! Cada fase recibe el `Document` acumulado y lo transforma antes de pasarlo
//! a la siguiente. Las dependencias son `Arc<dyn Trait>` para que el caller
//! en `app_state` las comparta sin clonar sesiones ONNX ni diccionarios.

use crate::domain::{BlockType, Document, ProcessingProfile};
use crate::interfaces::ports::{
    DocumentParserPort, LayoutEnginePort, OcrEnginePort, PostprocessorPort,
    PreprocessorPort, TableAnalyzerPort,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Mensaje canonico de cancelacion por el usuario.
///
/// Compartido entre el pipeline (emisor) y `app_state` (receptor) para distinguir
/// cancelaciones de errores reales sin agregar un variante extra a `PipelineEvent`.
pub const MSG_JOB_CANCELADO: &str = "Job cancelado por el usuario";

/// Eventos producidos por el pipeline hacia la TUI via canal `mpsc`.
///
/// `FaseCambiada` y `ProgresoActualizado` son variantes separadas porque tienen
/// frecuencias de disparo distintas: el primero ocurre N veces (una por fase),
/// el segundo ocurre M*N veces (una por pagina dentro de cada fase iterativa).
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// Transicion a una nueva fase con su progreso global estimado en [0.0, 1.0].
    FaseCambiada { fase: String, progreso: f32 },

    /// Avance de pagina dentro de la fase activa.
    ProgresoActualizado { pagina_actual: u32, total_paginas: u32 },

    /// Documento procesado listo para exportar. Se envia por canal porque el
    /// pipeline corre en un thread separado sin acceso directo a `AppState`.
    Completado(Document),

    Error(String),
}

/// Pipeline OCR con dependencias inyectables por fase via builder.
pub struct OcrPipeline {
    parser: Arc<dyn DocumentParserPort>,
    preprocesador: Option<Arc<dyn PreprocessorPort>>,
    layout_engine: Option<Arc<dyn LayoutEnginePort>>,
    ocr_engine: Arc<dyn OcrEnginePort>,
    table_analyzer: Option<Arc<dyn TableAnalyzerPort>>,
    postprocesador: Option<Arc<dyn PostprocessorPort>>,
}

impl OcrPipeline {
    /// Crea el pipeline con las dependencias minimas: parser y motor OCR.
    pub fn new(
        parser: Arc<dyn DocumentParserPort>,
        ocr_engine: Arc<dyn OcrEnginePort>,
    ) -> Self {
        Self {
            parser,
            preprocesador: None,
            layout_engine: None,
            ocr_engine,
            table_analyzer: None,
            postprocesador: None,
        }
    }

    pub fn with_preprocessor(mut self, preprocesador: Arc<dyn PreprocessorPort>) -> Self {
        self.preprocesador = Some(preprocesador);
        self
    }

    pub fn with_layout_engine(mut self, layout_engine: Arc<dyn LayoutEnginePort>) -> Self {
        self.layout_engine = Some(layout_engine);
        self
    }

    pub fn with_table_analyzer(mut self, table_analyzer: Arc<dyn TableAnalyzerPort>) -> Self {
        self.table_analyzer = Some(table_analyzer);
        self
    }

    pub fn with_postprocessor(mut self, postprocesador: Arc<dyn PostprocessorPort>) -> Self {
        self.postprocesador = Some(postprocesador);
        self
    }

    /// Ejecuta las 6 fases en secuencia y retorna el documento procesado.
    ///
    /// El flag `cancelacion` se verifica entre fases (no dentro): garantiza abortar
    /// en un punto limpio sin matar el thread a la fuerza.
    pub fn procesar_documento(
        &self,
        ruta: &Path,
        perfil: &ProcessingProfile,
        notificador: Option<&std::sync::mpsc::Sender<PipelineEvent>>,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Document, Box<dyn std::error::Error + Send + Sync>> {
        // Fase 1: Parseo
        self.notificar(notificador, PipelineEvent::FaseCambiada {
            fase: "Parseando documento".to_string(),
            progreso: 0.0,
        });

        let mut documento = self.parser.parse(ruta)?;
        let total_paginas = documento.pages.len() as u32;
        log::info!("Pipeline: documento parseado ({} paginas)", total_paginas);

        self.verificar_cancelacion(cancelacion)?;

        // Fase 2: Preprocesamiento
        if let Some(ref preprocesador) = self.preprocesador {
            self.notificar(notificador, PipelineEvent::FaseCambiada {
                fase: "Preprocesando imagenes".to_string(),
                progreso: 0.15,
            });
            preprocesador.preprocess(&mut documento)?;
            log::info!("Pipeline: preprocesamiento completado");
        }

        self.verificar_cancelacion(cancelacion)?;

        // Fase 3: Layout
        if let Some(ref layout_engine) = self.layout_engine {
            self.notificar(notificador, PipelineEvent::FaseCambiada {
                fase: "Analizando layout".to_string(),
                progreso: 0.30,
            });
            for (i, pagina) in documento.pages.iter_mut().enumerate() {
                let bloques = layout_engine.analyze(pagina)?;
                pagina.blocks = bloques;
                self.notificar(notificador, PipelineEvent::ProgresoActualizado {
                    pagina_actual: (i + 1) as u32,
                    total_paginas,
                });
            }
            log::info!("Pipeline: layout completado ({})", layout_engine.name());
        }

        self.verificar_cancelacion(cancelacion)?;

        // Fase 4: OCR
        //
        // TODO(Fix 4): iterar por pagina con `process_page` para emitir
        // ProgresoActualizado por cada pagina y permitir cancelacion granular.
        self.notificar(notificador, PipelineEvent::FaseCambiada {
            fase: "Reconociendo texto (OCR)".to_string(),
            progreso: 0.50,
        });
        self.ocr_engine.process(&mut documento, perfil)?;
        log::info!("Pipeline: OCR completado ({})", self.ocr_engine.name());

        self.verificar_cancelacion(cancelacion)?;

        // Fase 5: Tablas
        if let Some(ref table_analyzer) = self.table_analyzer {
            // Table Transformer es costoso; activarlo solo si el layout detecto tablas.
            let hay_tablas = documento.pages.iter().any(|p| {
                p.blocks.iter().any(|b| b.block_type == BlockType::Table)
            });
            if hay_tablas {
                self.notificar(notificador, PipelineEvent::FaseCambiada {
                    fase: "Analizando tablas".to_string(),
                    progreso: 0.75,
                });
                table_analyzer.analyze_tables(&mut documento)?;
                log::info!("Pipeline: tablas completado ({})", table_analyzer.name());
            }
        }

        self.verificar_cancelacion(cancelacion)?;

        // Fase 6: Postprocesamiento
        if let Some(ref postprocesador) = self.postprocesador {
            self.notificar(notificador, PipelineEvent::FaseCambiada {
                fase: "Postprocesando texto".to_string(),
                progreso: 0.90,
            });
            postprocesador.postprocess(&mut documento)?;
            log::info!("Pipeline: postprocesamiento completado");
        }

        self.notificar(notificador, PipelineEvent::FaseCambiada {
            fase: "Completado".to_string(),
            progreso: 1.0,
        });

        Ok(documento)
    }

    fn verificar_cancelacion(
        &self,
        cancelacion: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Relaxed es suficiente: un solo escritor (TUI), la latencia de algunos
        // ciclos entre escritura y deteccion es aceptable aqui.
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
