use ocrfast::application::tui;
use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
use ocrfast::infrastructure::job_store::FileJobStore;
use ocrfast::infrastructure::ocr_engines::stub::StubOcrEngine;
use ocrfast::interfaces::ports::{DocumentParserPort, JobStorePort, OcrEnginePort};
use std::sync::Arc;

/// Inicializa logging dual a stderr y archivo local.
///
/// La ruta persiste en el directorio de datos del usuario para desacoplar la
/// observabilidad del directorio actual. Si el archivo no puede abrirse, la
/// aplicación sigue operativa y conserva visibilidad mínima por stderr.
fn configurar_logging() {
    let ruta_log = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ocrfast")
        .join("ocrfast.log");

    if let Some(padre) = ruta_log.parent() {
        let _ = std::fs::create_dir_all(padre);
    }

    let resultado_log = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stderr())
        .chain(
            fern::log_file(&ruta_log)
                .unwrap_or_else(|_| fern::log_file(std::path::Path::new("/dev/null")).unwrap()),
        )
        .apply();

    if let Err(e) = resultado_log {
        eprintln!("[ADVERTENCIA] No se pudo inicializar el sistema de logging: {}. Los logs solo iran a stderr.", e);
    }
}

/// Arranca la TUI con dependencias locales y fallback progresivo.
///
/// El proceso inicia siempre sobre un motor stub para entregar el primer frame
/// sin esperar carga de modelos ni probing de aceleradores. El backend ONNX se
/// incorpora después en segundo plano, salvo que el caller fuerce `--stub`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    configurar_logging();

    let usar_stubs_manual = std::env::args().any(|arg| arg == "--stub");

    log::info!("Iniciando OCRFast TUI...");

    let parser: Arc<dyn DocumentParserPort> = if usar_stubs_manual {
        Arc::new(StubDocumentParser::new())
    } else {
        use ocrfast::infrastructure::document_parsers::image_parser::ImageDocumentParser;
        Arc::new(ImageDocumentParser::new())
    };
    let ocr_engine: Arc<dyn OcrEnginePort> = Arc::new(StubOcrEngine::new());

    let job_store: Arc<dyn JobStorePort> = match FileJobStore::new() {
        Ok(store) => {
            log::info!("Usando FileJobStore: {:?}", store.ruta());
            Arc::new(store)
        }
        Err(e) => {
            log::warn!(
                "No se pudo inicializar FileJobStore ({}). Usando almacen en memoria.",
                e
            );
            use ocrfast::infrastructure::job_store::InMemoryJobStore;
            Arc::new(InMemoryJobStore::new())
        }
    };

    let cargar_onnx = !usar_stubs_manual;

    tui::run(parser, ocr_engine, job_store, cargar_onnx)?;

    log::info!("OCRFast TUI finalizado.");
    Ok(())
}
