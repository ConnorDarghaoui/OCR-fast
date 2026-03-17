//! Tests de integracion del pipeline OCR completo sin modelos ONNX.
//!
//! Usan `StubDocumentParser` y `StubOcrEngine` para verificar el flujo
//! del pipeline sin dependencias externas. Se ejecutan en CI sin descargas.
//!
//! Ejecutar: `cargo test --test pipeline_integration_tests`

use ocrfast::application::pipeline::{OcrPipeline, PipelineEvent};
use ocrfast::domain::{Document, ProcessingProfile};
use ocrfast::domain::errors::OcrError;
use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
use ocrfast::interfaces::ports::{OcrEnginePort, PostprocessorPort};
use std::path::Path;
use std::sync::{mpsc, Arc};

// ---------------------------------------------------------------------------
// Stub OCR Engine para tests: simula reconocimiento de texto sin ONNX.
// ---------------------------------------------------------------------------

struct StubOcrEngine;

impl OcrEnginePort for StubOcrEngine {
    fn process(
        &self,
        document: &mut Document,
        _profile: &ProcessingProfile,
    ) -> Result<(), OcrError> {
        // Rellenar contenido de todos los bloques con texto simulado
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

// ---------------------------------------------------------------------------
// Postprocessor stub que registra si fue llamado.
// ---------------------------------------------------------------------------

struct PostprocesadorRegistrador {
    llamado: std::sync::Mutex<bool>,
}

impl PostprocesadorRegistrador {
    fn new() -> Self {
        Self { llamado: std::sync::Mutex::new(false) }
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

// ---------------------------------------------------------------------------
// Tests de integracion
// ---------------------------------------------------------------------------

/// Verifica el flujo completo del pipeline con stubs: parseo → OCR → resultado.
///
/// No requiere modelos ONNX ni archivos reales. StubDocumentParser genera
/// un Document sintetico y StubOcrEngine rellena el contenido de los bloques.
#[test]
fn test_pipeline_completo_con_stubs() {
    let parser = Arc::new(StubDocumentParser::new());
    let ocr = Arc::new(StubOcrEngine);

    let pipeline = OcrPipeline::new(parser, ocr);

    // StubDocumentParser acepta cualquier ruta; la extension .pdf activa el Parser
    let ruta = Path::new("/tmp/documento_test.pdf");
    let resultado = pipeline.procesar_documento(ruta, &ProcessingProfile::Balanced, None, None);

    assert!(
        resultado.is_ok(),
        "Pipeline con stubs debe completarse sin error: {:?}",
        resultado.err()
    );

    let doc = resultado.unwrap();
    assert!(!doc.pages.is_empty(), "El documento debe tener al menos 1 pagina");

    // Verificar que OCR rellenó el contenido de los bloques
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

    // El documento se retorna directamente por valor de retorno, no por canal
    assert!(!documento.pages.is_empty(), "El documento retornado debe tener paginas");

    // Recoger todos los eventos emitidos
    drop(tx); // cerrar el sender para que rx.iter() termine
    let eventos: Vec<PipelineEvent> = rx.iter().collect();

    assert!(
        !eventos.is_empty(),
        "El pipeline debe emitir al menos un evento PipelineEvent"
    );

    // Debe contener al menos un evento FaseCambiada
    let hay_fases = eventos.iter().any(|e| matches!(e, PipelineEvent::FaseCambiada { .. }));
    assert!(hay_fases, "Debe haberse emitido al menos un evento FaseCambiada");

    // El pipeline NO debe emitir PipelineEvent::Completado: es responsabilidad del caller
    let hay_completado = eventos.iter().any(|e| matches!(e, PipelineEvent::Completado(_)));
    assert!(!hay_completado, "El pipeline no debe emitir Completado internamente; es responsabilidad del caller");

    // El ultimo evento emitido por el pipeline debe ser FaseCambiada con progreso 1.0
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

    let pipeline = OcrPipeline::new(parser, ocr)
        .with_postprocessor(postprocesador);

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
