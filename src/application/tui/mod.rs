/// Estado reactivo y coordinación de jobs de la TUI.
pub mod app_state;
/// Coordinación del bootstrap ONNX fuera del estado renderizable.
mod engine_bootstrap;
/// Loop de eventos de teclado y mouse desacoplado del render.
pub mod events;
/// Coordinación de jobs OCR en background y su progreso visible.
mod job_runtime;
/// Render puro de widgets y composición visual.
pub mod ui;

use crate::interfaces::ports::{DocumentParserPort, JobStorePort, OcrEnginePort};
use app_state::{AppState, ViewMode};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::Arc;

/// Inicializa y ejecuta la aplicacion TUI.
///
/// El módulo toma dependencias por puertos para que la capa interactiva quede
/// desacoplada de motores reales, stubs y mecanismos concretos de persistencia.
///
/// # Notes
///
/// El arranque registra un hook de pánico que restaura el terminal antes de
/// delegar al handler original. Sin esa restauración, un `panic!` dejaría la
/// sesión en raw mode y con alternate screen activa.
fn registrar_hook_panico() {
    let hook_original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture,);
        hook_original(info);
    }));
}

/// Ejecuta la TUI completa y restaura el terminal al salir.
///
/// La función centraliza inicialización y teardown del terminal porque esa
/// responsabilidad no puede dispersarse sin riesgo de dejar la sesión del shell
/// en raw mode tras un panic o un retorno temprano.
///
/// # Errors
///
/// Retorna `io::Error` cuando la infraestructura de terminal no puede entrar en
/// modo alterno, raw mode o restaurarse correctamente.
///
/// # Notes
///
/// El hook de pánico se registra antes de habilitar raw mode para cubrir toda la
/// ventana crítica de la sesión interactiva.
pub fn run(
    parser: Arc<dyn DocumentParserPort>,
    ocr_engine: Arc<dyn OcrEnginePort>,
    job_store: Arc<dyn JobStorePort>,
    cargar_onnx: bool,
) -> Result<(), io::Error> {
    registrar_hook_panico();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState::nuevo(parser, ocr_engine, job_store);

    if cargar_onnx {
        app.iniciar_carga_motor();
    } else {
        app.marcar_motor_listo();
        app.vista_actual = ViewMode::JobList;
    }

    let res = events::ejecutar_bucle_eventos(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error en TUI: {:?}", err);
    }

    Ok(())
}
