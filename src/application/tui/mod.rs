/// Estado reactivo y coordinación de jobs de la TUI.
pub mod app_state;
/// Loop de eventos de teclado y mouse desacoplado del render.
pub mod events;
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
/// # Parametros
/// - `parser`: Puerto de parseador de documentos (inyectado).
/// - `ocr_engine`: Puerto de motor OCR (inyectado).
///
/// # Retorna
/// Resultado de la ejecucion o error de I/O.
/// Registra un hook de panico que restaura el terminal antes de abortar.
///
/// Sin este hook, cualquier `panic!` dentro del loop TUI deja la terminal
/// en raw mode con la pantalla alternativa activa, inutilizando la sesion
/// del shell del usuario.
fn registrar_hook_panico() {
    let hook_original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restaurar terminal incondicionalmente; los errores se ignoran porque
        // ya estamos en un camino de panico y no hay forma util de propagarlos.
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture,);
        // Delegar al handler original para que imprima el backtrace/mensaje.
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
    // Registrar antes de habilitar raw mode para cubrir cualquier fallo posterior.
    registrar_hook_panico();

    // Configurar terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Crear estado de la aplicacion
    let mut app = AppState::nuevo(parser, ocr_engine, job_store);

    // Iniciar carga en background si se solicita
    if cargar_onnx {
        app.iniciar_carga_motor();
    } else {
        // Si no cargamos ONNX, vamos directo a la lista
        app.motor_cargado = true;
        app.vista_actual = ViewMode::JobList;
    }

    // Loop principal de eventos y rendering
    let res = events::ejecutar_bucle_eventos(&mut terminal, &mut app);

    // Restaurar terminal
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
