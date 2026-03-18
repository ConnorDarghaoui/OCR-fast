use super::app_state::{AppState, InputMode, ViewMode};
use super::ui;
use crate::domain::OutputFormat;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{backend::Backend, Terminal};
use std::io;
use std::time::Duration;

/// Ejecuta el loop principal de render y despacho de eventos.
///
/// El loop mantiene una frecuencia de sondeo moderada para equilibrar
/// responsividad de terminal con consumo de CPU. Toda mutación pasa por
/// `AppState`, de modo que el módulo conserva la disciplina de un único punto
/// de verdad y evita divergencia entre render y manejo de eventos.
///
/// # Errors
///
/// Propaga fallos de `crossterm` o del backend de `ratatui`.
pub fn ejecutar_bucle_eventos<B: Backend>(
    terminal: &mut Terminal<B>,
    aplicacion: &mut AppState,
) -> Result<(), io::Error> {
    loop {
        terminal.draw(|f| ui::renderizar_interfaz(f, aplicacion))?;

        if aplicacion.debe_salir {
            break;
        }

        aplicacion.consultar_trabajos();

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(tecla) => manejar_evento_tecla(tecla, aplicacion)?,
                Event::Mouse(raton) => manejar_evento_raton(raton, aplicacion)?,
                _ => {}
            }
        }
    }

    Ok(())
}

/// Enruta una pulsación según el modo de entrada activo.
///
/// La selección de formato se trata como modal duro para preservar consistencia:
/// mientras el popup está visible, no se permite que atajos globales muten el
/// resto del estado de la TUI.
fn manejar_evento_tecla(tecla: KeyEvent, aplicacion: &mut AppState) -> Result<(), io::Error> {
    if tecla.kind != KeyEventKind::Press {
        return Ok(());
    }

    if aplicacion.seleccionando_formato {
        return manejar_seleccion_formato(tecla, aplicacion);
    }

    match aplicacion.modo_entrada {
        InputMode::Normal => manejar_modo_normal(tecla, aplicacion),
        InputMode::Editing => manejar_modo_edicion(tecla, aplicacion),
    }
}

/// Opera el popup modal de formato de salida.
fn manejar_seleccion_formato(tecla: KeyEvent, app: &mut AppState) -> Result<(), io::Error> {
    match tecla.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.indice_formato = (app.indice_formato + 1) % OutputFormat::OPCIONES.len();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let n = OutputFormat::OPCIONES.len();
            app.indice_formato = (app.indice_formato + n - 1) % n;
        }
        KeyCode::Enter => {
            if let Err(e) = app.confirmar_formato() {
                app.mostrar_estado(format!("Error: {}", e));
            }
        }
        KeyCode::Esc => {
            app.seleccionando_formato = false;
            app.ruta_pendiente = None;
            app.indice_formato = 0;
        }
        _ => {}
    }
    Ok(())
}

/// Gestiona atajos cuando la TUI no está capturando texto libre.
fn manejar_modo_normal(tecla: KeyEvent, aplicacion: &mut AppState) -> Result<(), io::Error> {
    if tecla.code == KeyCode::Char('?') && aplicacion.vista_actual != ViewMode::Help {
        aplicacion.cambiar_vista(ViewMode::Help);
        return Ok(());
    }

    match aplicacion.vista_actual {
        ViewMode::Initializing => match tecla.code {
            KeyCode::Char('q') => aplicacion.salir(),
            _ => {}
        },
        ViewMode::JobList => match tecla.code {
            KeyCode::Char('q') => aplicacion.salir(),
            KeyCode::Char('n') => aplicacion.iniciar_agregar_archivo(),
            KeyCode::Down | KeyCode::Char('j') => aplicacion.seleccionar_siguiente(),
            KeyCode::Up | KeyCode::Char('k') => aplicacion.seleccionar_anterior(),
            KeyCode::Enter => {
                if aplicacion.obtener_trabajo_seleccionado().is_some() {
                    aplicacion.cambiar_vista(ViewMode::JobDetail);
                }
            }
            KeyCode::Char('s') => aplicacion.cambiar_vista(ViewMode::Settings),
            KeyCode::Char('x') => aplicacion.eliminar_trabajo_seleccionado(),
            KeyCode::Char('c') => aplicacion.limpiar_trabajos_finalizados(),
            KeyCode::Char('z') => aplicacion.cancelar_trabajo_seleccionado(),
            _ => {}
        },
        ViewMode::JobDetail => match tecla.code {
            KeyCode::Char('q') | KeyCode::Esc => aplicacion.cambiar_vista(ViewMode::JobList),
            KeyCode::Char('t') => aplicacion.exportar_trabajo_txt(),
            KeyCode::Char('d') => aplicacion.exportar_trabajo_docx(),
            KeyCode::Char('l') => aplicacion.exportar_trabajo_latex(),
            KeyCode::Char('J') => aplicacion.exportar_trabajo_json(),
            KeyCode::Char('p') => aplicacion.exportar_trabajo_pdf(),
            KeyCode::Down | KeyCode::Char('j') => aplicacion.scroll_detalle_abajo(),
            KeyCode::Up | KeyCode::Char('k') => aplicacion.scroll_detalle_arriba(),
            KeyCode::Char('z') => aplicacion.cancelar_trabajo_seleccionado(),
            _ => {}
        },
        ViewMode::Settings => match tecla.code {
            KeyCode::Char('q') | KeyCode::Esc => aplicacion.cambiar_vista(ViewMode::JobList),
            KeyCode::Char('1') => {
                aplicacion.perfil = crate::domain::ProcessingProfile::Fast;
                aplicacion.loguear("Perfil RAPIDO activado".to_string());
            }
            KeyCode::Char('2') => {
                aplicacion.perfil = crate::domain::ProcessingProfile::Balanced;
                aplicacion.loguear("Perfil EQUILIBRADO activado".to_string());
            }
            KeyCode::Char('3') => {
                aplicacion.perfil = crate::domain::ProcessingProfile::Accurate;
                aplicacion.loguear("Perfil PRECISO activado".to_string());
            }
            KeyCode::Char('4') => {
                aplicacion.idioma.primary = "spa".to_string();
                aplicacion.loguear("Idioma: Espanol (spa)".to_string());
            }
            KeyCode::Char('5') => {
                aplicacion.idioma.primary = "eng".to_string();
                aplicacion.loguear("Idioma: Ingles (eng)".to_string());
            }
            _ => {}
        },
        ViewMode::Help => match tecla.code {
            KeyCode::Char('q') | KeyCode::Esc => aplicacion.cambiar_vista(ViewMode::JobList),
            _ => {}
        },
    }

    Ok(())
}

/// Gestiona edición de rutas y otros campos de entrada lineal.
fn manejar_modo_edicion(tecla: KeyEvent, aplicacion: &mut AppState) -> Result<(), io::Error> {
    match tecla.code {
        KeyCode::Enter => {
            if let Err(e) = aplicacion.procesar_archivo_ingresado() {
                aplicacion.mostrar_estado(format!("Error: {}", e));
                aplicacion.loguear(format!("Error: {}", e));
            }
        }
        KeyCode::Esc => aplicacion.cancelar_edicion(),
        KeyCode::Char(c) => aplicacion.buffer_entrada.push(c),
        KeyCode::Backspace => {
            aplicacion.buffer_entrada.pop();
        }
        _ => {}
    }

    Ok(())
}

/// Mapea eventos de ratón a navegación contextual de listas y detalle.
fn manejar_evento_raton(
    raton: crossterm::event::MouseEvent,
    aplicacion: &mut AppState,
) -> Result<(), io::Error> {
    use crossterm::event::MouseEventKind;

    if aplicacion.modo_entrada != InputMode::Normal {
        return Ok(());
    }

    match raton.kind {
        MouseEventKind::ScrollDown => match aplicacion.vista_actual {
            ViewMode::JobList => aplicacion.seleccionar_siguiente(),
            ViewMode::JobDetail => aplicacion.scroll_detalle_abajo(),
            _ => {}
        },
        MouseEventKind::ScrollUp => match aplicacion.vista_actual {
            ViewMode::JobList => aplicacion.seleccionar_anterior(),
            ViewMode::JobDetail => aplicacion.scroll_detalle_arriba(),
            _ => {}
        },
        _ => {}
    }

    Ok(())
}
