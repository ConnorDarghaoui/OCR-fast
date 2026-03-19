use ocrfast::application::pipeline::{
    NoopRefinementPass, OcrPipeline, PipelineEvent, PipelineFailure, RefinementBudget,
    RefinementContext, RefinementPass, RefinementStage,
};
use ocrfast::application::pipeline::recovery::ConfidenceBoostPass;
use ocrfast::domain::errors::DocumentError;
use ocrfast::domain::errors::{LayoutError, OcrError};
use ocrfast::domain::{
    Block, BlockType, Dimensions, Document, DocumentBlueprint, Page, ProcessingProfile, Rectangle,
};
use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
use ocrfast::interfaces::ports::{
    DocumentAssemblerPort, DocumentParserPort, OcrEnginePort, PostprocessorPort,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

struct StubOcrEngine;

impl OcrEnginePort for StubOcrEngine {
    fn process(
        &self,
        document: &mut Document,
        _profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        for pagina in &mut document.pages {
            for (i, bloque) in pagina.blocks.iter_mut().enumerate() {
                bloque.content = format!("Texto OCR simulado bloque {}", i + 1);
                bloque.confidence = 0.92;
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "StubOcrEngine"
    }
}

struct PostprocesadorRegistrador {
    llamado: std::sync::Mutex<bool>,
}

impl PostprocesadorRegistrador {
    fn new() -> Self {
        Self {
            llamado: std::sync::Mutex::new(false),
        }
    }

    fn fue_llamado(&self) -> bool {
        *self.llamado.lock().unwrap()
    }
}

impl PostprocessorPort for PostprocesadorRegistrador {
    fn postprocess(&self, _document: &mut Document) -> Result<(), OcrError> {
        *self.llamado.lock().unwrap() = true;
        Ok(())
    }
}

struct PostprocesadorMayusculas;

impl PostprocessorPort for PostprocesadorMayusculas {
    fn postprocess(&self, document: &mut Document) -> Result<(), OcrError> {
        for pagina in &mut document.pages {
            for bloque in &mut pagina.blocks {
                bloque.content = bloque.content.to_uppercase();
            }
        }
        Ok(())
    }
}

struct RefinamientoRegistrador {
    stage: RefinementStage,
    nombre: &'static str,
    llamado: std::sync::Mutex<u32>,
    vio_blueprint: std::sync::Mutex<bool>,
}

impl RefinamientoRegistrador {
    fn new(stage: RefinementStage, nombre: &'static str) -> Self {
        Self {
            stage,
            nombre,
            llamado: std::sync::Mutex::new(0),
            vio_blueprint: std::sync::Mutex::new(false),
        }
    }

    fn llamadas(&self) -> u32 {
        *self.llamado.lock().unwrap()
    }

    fn vio_blueprint(&self) -> bool {
        *self.vio_blueprint.lock().unwrap()
    }
}

impl RefinementPass for RefinamientoRegistrador {
    fn stage(&self) -> RefinementStage {
        self.stage
    }

    fn name(&self) -> &str {
        self.nombre
    }

    fn refine(
        &self,
        document: &mut Document,
        blueprint: &mut Option<DocumentBlueprint>,
        context: &RefinementContext<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.llamado.lock().unwrap() += 1;
        if blueprint.is_some() {
            *self.vio_blueprint.lock().unwrap() = true;
        }
        document.metadata.insert(
            format!("refinement:{}", self.nombre),
            format!(
                "{:?}:{}:{}",
                context.stage, context.consumed_passes, context.remaining_passes
            ),
        );
        Ok(())
    }
}

struct ParserBloquesDebiles;

impl DocumentParserPort for ParserBloquesDebiles {
    fn parse(&self, path: &Path) -> Result<Document, DocumentError> {
        Ok(Document {
            id: "doc-low-confidence".to_string(),
            source_path: path.to_path_buf(),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 1200,
                    height: 1600,
                },
                blocks: vec![
                    Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 100,
                            y: 120,
                            width: 500,
                            height: 140,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        layout_confidence: None,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    },
                    Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 100,
                            y: 320,
                            width: 500,
                            height: 140,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        layout_confidence: None,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 1,
                    },
                ],
                image_data: None,
            }],
            metadata: HashMap::new(),
        })
    }
}

struct OcrEngineDeRefuerzo;

impl OcrEnginePort for OcrEngineDeRefuerzo {
    fn process(
        &self,
        document: &mut Document,
        profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        let bloques = &mut document.pages[0].blocks;
        match profile {
            ProcessingProfile::Balanced => {
                bloques[0].content = "texto borroso".to_string();
                bloques[0].confidence = 0.42;
                bloques[1].content = "texto estable".to_string();
                bloques[1].confidence = 0.93;
            }
            ProcessingProfile::Accurate => {
                bloques[0].content = "texto corregido".to_string();
                bloques[0].confidence = 0.89;
                bloques[1].content = "texto estable".to_string();
                bloques[1].confidence = 0.94;
            }
            ProcessingProfile::Fast => {
                bloques[0].content = "texto rapido".to_string();
                bloques[0].confidence = 0.35;
                bloques[1].content = "texto estable".to_string();
                bloques[1].confidence = 0.80;
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "OcrEngineDeRefuerzo"
    }
}

struct ParserPaginasMixtas;

impl DocumentParserPort for ParserPaginasMixtas {
    fn parse(&self, path: &Path) -> Result<Document, DocumentError> {
        Ok(Document {
            id: "doc-mixed-confidence".to_string(),
            source_path: path.to_path_buf(),
            pages: vec![
                Page {
                    number: 1,
                    dimensions: Dimensions {
                        width: 800,
                        height: 1200,
                    },
                    blocks: vec![Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 80,
                            y: 100,
                            width: 500,
                            height: 120,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        layout_confidence: None,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    }],
                    image_data: None,
                },
                Page {
                    number: 2,
                    dimensions: Dimensions {
                        width: 800,
                        height: 1200,
                    },
                    blocks: vec![Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 90,
                            y: 160,
                            width: 480,
                            height: 120,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        layout_confidence: None,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    }],
                    image_data: None,
                },
            ],
            metadata: HashMap::new(),
        })
    }
}

struct OcrEngineRegistraReintento {
    paginas_reintentadas: std::sync::Mutex<Vec<usize>>,
}

impl OcrEngineRegistraReintento {
    fn new() -> Self {
        Self {
            paginas_reintentadas: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn paginas_reintentadas(&self) -> Vec<usize> {
        self.paginas_reintentadas.lock().unwrap().clone()
    }
}

impl OcrEnginePort for OcrEngineRegistraReintento {
    fn process(
        &self,
        document: &mut Document,
        profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        if *profile == ProcessingProfile::Accurate {
            self.paginas_reintentadas
                .lock()
                .unwrap()
                .push(document.pages.len());
        }

        for pagina in &mut document.pages {
            let bloque = &mut pagina.blocks[0];
            match (*profile, pagina.number) {
                (ProcessingProfile::Balanced, 1) => {
                    bloque.content = "pagina debil".to_string();
                    bloque.confidence = 0.42;
                }
                (ProcessingProfile::Balanced, 2) => {
                    bloque.content = "pagina estable".to_string();
                    bloque.confidence = 0.95;
                }
                (ProcessingProfile::Accurate, 1) => {
                    bloque.content = "pagina corregida".to_string();
                    bloque.confidence = 0.90;
                }
                (ProcessingProfile::Accurate, 2) => {
                    bloque.content = "pagina estable".to_string();
                    bloque.confidence = 0.95;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "OcrEngineRegistraReintento"
    }
}

struct PassCancela {
    cancelacion: Arc<AtomicBool>,
}

impl RefinementPass for PassCancela {
    fn stage(&self) -> RefinementStage {
        RefinementStage::AfterBlueprint
    }

    fn name(&self) -> &str {
        "PassCancela"
    }

    fn refine(
        &self,
        _document: &mut Document,
        _blueprint: &mut Option<DocumentBlueprint>,
        _context: &RefinementContext<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.cancelacion.store(true, Ordering::Relaxed);
        Ok(())
    }
}

struct EnsambladorRegistrador {
    llamado: std::sync::Mutex<bool>,
}

impl EnsambladorRegistrador {
    fn new() -> Self {
        Self {
            llamado: std::sync::Mutex::new(false),
        }
    }

    fn fue_llamado(&self) -> bool {
        *self.llamado.lock().unwrap()
    }
}

impl DocumentAssemblerPort for EnsambladorRegistrador {
    fn assemble(&self, document: &mut Document) -> Result<(), LayoutError> {
        *self.llamado.lock().unwrap() = true;

        for pagina in &mut document.pages {
            pagina.blocks.reverse();
            for (indice, bloque) in pagina.blocks.iter_mut().enumerate() {
                bloque.reading_order = indice as u32;
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "EnsambladorRegistrador"
    }
}

/// Verifica el flujo completo del pipeline con stubs: parseo → OCR → resultado.
///
/// No requiere modelos ONNX ni archivos reales. StubDocumentParser genera
/// un Document sintetico y StubOcrEngine rellena el contenido de los bloques.
#[test]
fn test_pipeline_completo_con_stubs() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);

    let pipeline = OcrPipeline::new(parser, ocr);

    let ruta = Path::new("/tmp/documento_test.pdf");
    let resultado = pipeline.procesar_documento(ruta, &ProcessingProfile::Balanced, None, None);

    assert!(
        resultado.is_ok(),
        "Pipeline con stubs debe completarse sin error: {:?}",
        resultado.err()
    );

    let doc = resultado.unwrap();
    assert!(
        !doc.pages.is_empty(),
        "El documento debe tener al menos 1 pagina"
    );

    let bloques_con_contenido: usize = doc
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .filter(|b| !b.content.is_empty())
        .count();

    assert!(
        bloques_con_contenido > 0,
        "El StubOcrEngine debe haber procesado al menos 1 bloque"
    );
}

/// Verifica que el pipeline emite eventos PipelineEvent por el canal mpsc.
///
/// Contrato del pipeline tras refactor (eliminacion de double-comunicacion):
/// - Emite eventos FaseCambiada durante el procesamiento (progreso).
/// - NO emite PipelineEvent::Completado internamente: ese evento es
///   responsabilidad del caller (app_state) una vez recibe Ok(Document).
/// - El ultimo evento de fase debe ser FaseCambiada { fase: "Completado", progreso: 1.0 }.
/// - El resultado del documento se obtiene del valor de retorno Ok(Document), no del canal.
#[test]
fn test_pipeline_emite_eventos_de_fase() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let pipeline = OcrPipeline::new(parser, ocr);

    let (tx, rx) = mpsc::channel::<PipelineEvent>();
    let ruta = Path::new("/tmp/doc_eventos.pdf");

    let documento = pipeline
        .procesar_documento(ruta, &ProcessingProfile::Fast, Some(&tx), None)
        .expect("Pipeline debe completarse");

    assert!(
        !documento.pages.is_empty(),
        "El documento retornado debe tener paginas"
    );

    drop(tx);
    let eventos: Vec<PipelineEvent> = rx.iter().collect();

    assert!(
        !eventos.is_empty(),
        "El pipeline debe emitir al menos un evento PipelineEvent"
    );

    let hay_fases = eventos
        .iter()
        .any(|e| matches!(e, PipelineEvent::FaseCambiada { .. }));
    assert!(
        hay_fases,
        "Debe haberse emitido al menos un evento FaseCambiada"
    );

    let hay_completado = eventos
        .iter()
        .any(|e| matches!(e, PipelineEvent::Completado(_)));
    assert!(
        !hay_completado,
        "El pipeline no debe emitir Completado internamente; es responsabilidad del caller"
    );

    let ultimo = eventos.last().expect("Debe existir al menos un evento");
    assert!(
        matches!(ultimo, PipelineEvent::FaseCambiada { progreso, .. } if (*progreso - 1.0_f32).abs() < f32::EPSILON),
        "El ultimo evento del pipeline debe ser FaseCambiada con progreso 1.0"
    );
}

/// Verifica que el postprocesador configurado con `with_postprocessor`
/// es invocado tras la fase de OCR.
#[test]
fn test_pipeline_invoca_postprocesador() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let postprocesador = Arc::new(PostprocesadorRegistrador::new());
    let postprocesador_ref = Arc::clone(&postprocesador);

    let pipeline = OcrPipeline::new(parser, ocr).with_postprocessor(postprocesador);

    let ruta = Path::new("/tmp/doc_postproc.pdf");
    pipeline
        .procesar_documento(ruta, &ProcessingProfile::Accurate, None, None)
        .expect("Pipeline debe completarse");

    assert!(
        postprocesador_ref.fue_llamado(),
        "El postprocesador debe haber sido invocado durante el pipeline"
    );
}

/// Verifica que la confianza de los bloques OCR esta en el rango [0.0, 1.0].
#[test]
fn test_pipeline_confianza_en_rango_valido() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let pipeline = OcrPipeline::new(parser, ocr);

    let ruta = Path::new("/tmp/doc_confianza.png");
    let doc = pipeline
        .procesar_documento(ruta, &ProcessingProfile::Balanced, None, None)
        .expect("Pipeline debe completarse");

    for pagina in &doc.pages {
        for bloque in &pagina.blocks {
            assert!(
                bloque.confidence >= 0.0 && bloque.confidence <= 1.0,
                "Confianza del bloque debe estar en [0.0, 1.0], fue: {}",
                bloque.confidence
            );
        }
    }
}

#[test]
fn test_pipeline_invoca_refinement_pass_despues_del_blueprint() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let refinamiento = Arc::new(RefinamientoRegistrador::new(
        RefinementStage::AfterBlueprint,
        "after-blueprint",
    ));
    let refinamiento_ref = Arc::clone(&refinamiento);

    let pipeline = OcrPipeline::new(parser, ocr)
        .with_refinement_pass(refinamiento)
        .with_refinement_budget(RefinementBudget::new(2));

    let resultado = pipeline
        .procesar_documento_con_blueprint(
            Path::new("/tmp/doc_refinement_after_blueprint.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe completar refinamiento tras blueprint");

    assert!(resultado.blueprint.is_some());
    assert_eq!(refinamiento_ref.llamadas(), 1);
    assert!(refinamiento_ref.vio_blueprint());
    assert_eq!(
        resultado
            .document
            .metadata
            .get("refinement:after-blueprint")
            .map(String::as_str),
        Some("AfterBlueprint:0:2")
    );
}

#[test]
fn test_pipeline_refinement_budget_limita_passes() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let primer_pass = Arc::new(RefinamientoRegistrador::new(
        RefinementStage::AfterBlueprint,
        "primero",
    ));
    let segundo_pass = Arc::new(RefinamientoRegistrador::new(
        RefinementStage::AfterBlueprint,
        "segundo",
    ));
    let primer_ref = Arc::clone(&primer_pass);
    let segundo_ref = Arc::clone(&segundo_pass);

    let pipeline = OcrPipeline::new(parser, ocr)
        .with_refinement_pass(primer_pass)
        .with_refinement_pass(segundo_pass)
        .with_refinement_budget(RefinementBudget::new(1));

    pipeline
        .procesar_documento_con_blueprint(
            Path::new("/tmp/doc_refinement_budget.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe respetar presupuesto de refinamiento");

    assert_eq!(primer_ref.llamadas(), 1);
    assert_eq!(segundo_ref.llamadas(), 0);
}

#[test]
fn test_pipeline_invoca_refinement_pass_antes_del_blueprint_sin_builder() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let refinamiento = Arc::new(RefinamientoRegistrador::new(
        RefinementStage::BeforeBlueprint,
        "before-blueprint",
    ));
    let refinamiento_ref = Arc::clone(&refinamiento);

    let pipeline = OcrPipeline::new(parser, ocr)
        .with_refinement_pass(refinamiento)
        .with_refinement_budget(RefinementBudget::new(1));

    let documento = pipeline
        .procesar_documento(
            Path::new("/tmp/doc_refinement_before_blueprint.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe permitir refinamiento sin blueprint");

    assert_eq!(refinamiento_ref.llamadas(), 1);
    assert!(!refinamiento_ref.vio_blueprint());
    assert_eq!(
        documento
            .metadata
            .get("refinement:before-blueprint")
            .map(String::as_str),
        Some("BeforeBlueprint:0:1")
    );
}

#[test]
fn test_pipeline_admite_noop_refinement_pass() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let pipeline = OcrPipeline::new(parser, ocr)
        .with_refinement_pass(Arc::new(NoopRefinementPass::default()))
        .with_refinement_budget(RefinementBudget::new(1));

    let resultado = pipeline
        .procesar_documento_con_blueprint(
            Path::new("/tmp/doc_refinement_noop.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("El NoopRefinementPass no debe romper la corrida");

    assert!(resultado.blueprint.is_some());
}

#[test]
fn test_pipeline_con_blueprint_usa_page_composer_sin_builder_legacy() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let pipeline = OcrPipeline::new(parser, ocr);

    let resultado = pipeline
        .procesar_documento_con_blueprint(
            Path::new("/tmp/doc_blueprint_page_composer.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("El pipeline debe producir blueprint usando PageComposer");

    let blueprint = resultado
        .blueprint
        .expect("La ruta canónica debe producir blueprint aun sin builder legacy");

    assert_eq!(blueprint.document_id, resultado.document.id);
    assert_eq!(blueprint.pages.len(), resultado.document.pages.len());
    assert_eq!(
        blueprint.pages[0].elements.len(),
        resultado.document.pages[0].blocks.len()
    );
}

#[test]
fn test_pipeline_confidence_boost_mejora_solo_bloques_debiles() {
    let parser = Arc::new(ParserBloquesDebiles);
    let ocr: Arc<dyn OcrEnginePort> = Arc::new(OcrEngineDeRefuerzo);

    let pipeline = OcrPipeline::new(parser, Arc::clone(&ocr))
        .with_refinement_pass(Arc::new(ConfidenceBoostPass::with_config(
            ocr,
            0.78,
            0.05,
            ProcessingProfile::Accurate,
        )))
        .with_refinement_budget(RefinementBudget::new(1));

    let documento = pipeline
        .procesar_documento(
            Path::new("/tmp/doc_refinement_confidence_boost.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe aplicar el refuerzo OCR");

    assert_eq!(documento.pages[0].blocks[0].content, "texto corregido");
    assert!(
        (documento.pages[0].blocks[0].confidence - 0.89).abs() < f64::EPSILON,
        "El bloque debil debe adoptar el OCR de mayor confianza"
    );
    assert_eq!(documento.pages[0].blocks[1].content, "texto estable");
    assert!(
        (documento.pages[0].blocks[1].confidence - 0.93).abs() < f64::EPSILON,
        "El bloque ya confiable no debe reescribirse por una mejora marginal"
    );
}

#[test]
fn test_pipeline_confidence_boost_se_postprocesa_despues_del_reintento() {
    let parser = Arc::new(ParserBloquesDebiles);
    let ocr: Arc<dyn OcrEnginePort> = Arc::new(OcrEngineDeRefuerzo);

    let pipeline = OcrPipeline::new(parser, Arc::clone(&ocr))
        .with_postprocessor(Arc::new(PostprocesadorMayusculas))
        .with_refinement_pass(Arc::new(ConfidenceBoostPass::with_config(
            ocr,
            0.78,
            0.05,
            ProcessingProfile::Accurate,
        )))
        .with_refinement_budget(RefinementBudget::new(1));

    let documento = pipeline
        .procesar_documento(
            Path::new("/tmp/doc_refinement_postprocess.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe postprocesar el contenido reintentado");

    assert_eq!(documento.pages[0].blocks[0].content, "TEXTO CORREGIDO");
    assert_eq!(documento.pages[0].blocks[1].content, "TEXTO ESTABLE");
}

#[test]
fn test_pipeline_confidence_boost_reintenta_solo_paginas_debiles() {
    let parser = Arc::new(ParserPaginasMixtas);
    let ocr = Arc::new(OcrEngineRegistraReintento::new());
    let ocr_port: Arc<dyn OcrEnginePort> = ocr.clone();

    let pipeline = OcrPipeline::new(parser, Arc::clone(&ocr_port))
        .with_refinement_pass(Arc::new(ConfidenceBoostPass::with_config(
            ocr_port,
            0.78,
            0.05,
            ProcessingProfile::Accurate,
        )))
        .with_refinement_budget(RefinementBudget::new(1));

    let documento = pipeline
        .procesar_documento(
            Path::new("/tmp/doc_refinement_retry_pages.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("Pipeline debe reintentar solo paginas debiles");

    assert_eq!(ocr.paginas_reintentadas(), vec![1]);
    assert_eq!(documento.pages[0].blocks[0].content, "pagina corregida");
    assert_eq!(documento.pages[1].blocks[0].content, "pagina estable");
}

#[test]
fn test_pipeline_cancelacion_se_observa_entre_refinamientos() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let cancelacion = Arc::new(AtomicBool::new(false));
    let segundo_pass = Arc::new(RefinamientoRegistrador::new(
        RefinementStage::AfterBlueprint,
        "no-debe-correr",
    ));
    let segundo_ref = Arc::clone(&segundo_pass);

    let pipeline = OcrPipeline::new(parser, ocr)
        .with_refinement_pass(Arc::new(PassCancela {
            cancelacion: Arc::clone(&cancelacion),
        }))
        .with_refinement_pass(segundo_pass)
        .with_refinement_budget(RefinementBudget::new(2));

    let error = match pipeline.procesar_documento_con_blueprint(
        Path::new("/tmp/doc_refinement_cancel_between_passes.pdf"),
        &ProcessingProfile::Balanced,
        None,
        Some(&cancelacion),
    ) {
        Ok(_) => panic!("La cancelacion entre passes debe abortar la corrida"),
        Err(error) => error,
    };

    assert_eq!(error, PipelineFailure::Cancelado);
    assert_eq!(segundo_ref.llamadas(), 0);
}

#[test]
fn test_pipeline_invoca_ensamblador_documento() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);
    let ensamblador = Arc::new(EnsambladorRegistrador::new());
    let ensamblador_ref = Arc::clone(&ensamblador);

    let pipeline = OcrPipeline::new(parser, ocr).with_document_assembler(ensamblador);

    let ruta = Path::new("/tmp/doc_assembler.pdf");
    pipeline
        .procesar_documento(ruta, &ProcessingProfile::Balanced, None, None)
        .expect("Pipeline debe completar ensamblado");

    assert!(
        ensamblador_ref.fue_llamado(),
        "El ensamblador final debe ejecutarse al final del pipeline"
    );
}
