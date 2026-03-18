use ocrfast::application::pipeline::{
    NoopRefinementPass, OcrPipeline, PipelineEvent, RefinementBudget, RefinementContext,
    RefinementPass, RefinementStage,
};
use ocrfast::domain::errors::OcrError;
use ocrfast::domain::{Document, DocumentBlueprint, ProcessingProfile};
use ocrfast::infrastructure::document_blueprints::HighFidelityBlueprintBuilder;
use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
use ocrfast::interfaces::ports::{OcrEnginePort, PostprocessorPort};
use std::path::Path;
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
        .with_blueprint_builder(Arc::new(HighFidelityBlueprintBuilder::new()))
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
        .with_blueprint_builder(Arc::new(HighFidelityBlueprintBuilder::new()))
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
        .with_blueprint_builder(Arc::new(HighFidelityBlueprintBuilder::new()))
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
