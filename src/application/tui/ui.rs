use super::app_state::{AppState, InputMode, ViewMode};
use crate::domain::{JobStatus, OutputFormat, ProcessingProfile};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// Renderiza un frame completo a partir del `AppState` actual.
///
/// La función es deliberadamente pura respecto al estado visible: no muta la
/// aplicación y solo traduce datos a widgets. Esa separación hace que el render
/// sea testeable por snapshot y evita que lógica de negocio se mezcle con layout.
///
/// # Performance
///
/// El frame se compone con layouts deterministas y sin asignaciones de gran
/// volumen por tick, manteniendo estable el coste del redraw.
pub fn renderizar_interfaz(marco: &mut Frame, aplicacion: &AppState) {
    let hay_flash = aplicacion.obtener_estado().is_some();
    let motor_degradado = aplicacion.motor_fallido();
    let alto_banner_motor: u16 = if motor_degradado { 1 } else { 0 };

    let mut constraints = vec![
        Constraint::Length(3),                 // Encabezado pestanas
        Constraint::Length(alto_banner_motor), // Banner motor_fallido (0 o 1)
        Constraint::Min(0),                    // Centro
    ];
    if hay_flash {
        constraints.push(Constraint::Length(1)); // Barra de estado flash
    }
    constraints.push(Constraint::Length(7)); // Registros/Logs

    let layout_principal = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(marco.area());

    let idx_banner_motor = 1usize;
    let idx_centro = 2usize;
    let idx_flash = if hay_flash { Some(3usize) } else { None };
    let idx_logs = if hay_flash { 4usize } else { 3usize };

    renderizar_pestanas_encabezado(marco, aplicacion, layout_principal[0]);

    if motor_degradado {
        let barra = Paragraph::new(Span::styled(
            "  ADVERTENCIA: Motor ONNX no disponible. Resultados FICTICIOS (modo demostración). \
             Los archivos procesados no tendran texto real.",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        marco.render_widget(barra, layout_principal[idx_banner_motor]);
    }

    let layout_central = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Barra lateral
            Constraint::Percentage(75), // Contenido
        ])
        .split(layout_principal[idx_centro]);

    renderizar_monitor_lateral(marco, aplicacion, layout_central[0]);

    match aplicacion.vista_actual {
        ViewMode::Initializing => renderizar_inicializacion(marco, aplicacion, layout_central[1]),
        ViewMode::JobList | ViewMode::JobDetail => {
            renderizar_contenido_principal(marco, aplicacion, layout_central[1])
        }
        ViewMode::Settings => renderizar_configuracion(marco, aplicacion, layout_central[1]),
        ViewMode::Help => renderizar_ayuda(marco, layout_central[1]),
    }

    if let Some(flash_idx) = idx_flash {
        if let Some(msg) = aplicacion.obtener_estado() {
            let es_error = msg.starts_with("Error") || msg.starts_with("ERROR");
            let color = if es_error { Color::Red } else { Color::Green };
            let barra = Paragraph::new(Span::styled(
                format!("  {}", msg),
                Style::default()
                    .fg(Color::White)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ));
            marco.render_widget(barra, layout_principal[flash_idx]);
        }
    }

    renderizar_panel_registros(marco, aplicacion, layout_principal[idx_logs]);

    if aplicacion.modo_entrada == InputMode::Editing {
        renderizar_dialogo_entrada(marco, aplicacion);
    }

    if aplicacion.seleccionando_formato {
        renderizar_popup_formato(marco, aplicacion);
    }
}

/// Encabezado con sistema de pestañas
fn renderizar_pestanas_encabezado(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let titulos = vec![" [1] TRABAJOS ", " [2] AJUSTES ", " [?] AYUDA "];

    let indice = match aplicacion.vista_actual {
        ViewMode::JobList | ViewMode::JobDetail => 0,
        ViewMode::Settings => 1,
        ViewMode::Help => 2,
        _ => 0,
    };

    let pestanas = ratatui::widgets::Tabs::new(titulos)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(" OCRFast │ Sistema de Datos Terminal "),
        )
        .select(indice)
        .style(Style::default().fg(Color::Cyan))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    marco.render_widget(pestanas, area);
}

/// Barra Lateral Izquierda: Monitor de trabajos
fn renderizar_monitor_lateral(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let items: Vec<ListItem> = aplicacion
        .trabajos
        .iter()
        .enumerate()
        .map(|(i, trabajo)| {
            let (etiqueta, color) = obtener_etiqueta_y_color_estado(trabajo.status);
            let prefijo = if i == aplicacion.indice_seleccionado {
                "» "
            } else {
                "  "
            };

            let estilo = if i == aplicacion.indice_seleccionado {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let linea = Line::from(vec![
                Span::styled(prefijo, estilo),
                Span::styled(etiqueta, Style::default().fg(color)),
                Span::raw(format!(" {}", &trabajo.id[..8])),
            ]);

            ListItem::new(linea)
        })
        .collect();

    let lista = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Thick)
            .title(" MONITOR "),
    );

    marco.render_widget(lista, area);
}

/// Panel Inferior: Registro de sistema
fn renderizar_panel_registros(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let items_registros: Vec<ListItem> = aplicacion
        .registros
        .iter()
        .rev()
        .take(5)
        .map(|reg| {
            ListItem::new(Line::from(vec![
                Span::styled("  SISTEMA › ", Style::default().fg(Color::DarkGray)),
                Span::raw(reg),
            ]))
        })
        .collect();

    let lista_registros = List::new(items_registros).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Double)
            .title(" REGISTRO_SISTEMA "),
    );

    marco.render_widget(lista_registros, area);
}

/// Contenido principal central
fn renderizar_contenido_principal(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let trabajo = match aplicacion.obtener_trabajo_seleccionado() {
        Some(t) => t,
        None => {
            let parrafo =
                Paragraph::new("\n\n  ESPERANDO DATOS...\n\n  Presione 'n' para cargar archivo.")
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL).title(" TERMINAL "));
            marco.render_widget(parrafo, area);
            return;
        }
    };

    if aplicacion.vista_actual == ViewMode::JobList {
        renderizar_resumen_trabajo(marco, trabajo, area);
    } else {
        renderizar_detalle_trabajo_enmarcado(marco, aplicacion, trabajo, area);
    }
}

fn renderizar_resumen_trabajo(marco: &mut Frame, trabajo: &crate::domain::Job, area: Rect) {
    let (etiqueta, color) = obtener_etiqueta_y_color_estado(trabajo.status);

    let mut texto = vec![
        Line::from(vec![
            Span::styled(
                " ID_TRABAJO: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(&trabajo.id),
        ]),
        Line::from(vec![
            Span::styled(
                " ESTADO:      ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:?} {}", trabajo.status, etiqueta),
                Style::default().fg(color),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                " ORIGEN:      ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(trabajo.document.source_path.to_string_lossy()),
        ]),
    ];

    if trabajo.status == JobStatus::Failed {
        if let Some(ref msg) = trabajo.error_message {
            texto.push(Line::from(vec![
                Span::styled(
                    " ERROR:       ",
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
                ),
                Span::styled(msg.as_str(), Style::default().fg(Color::Red)),
            ]));
        }
    }

    texto.push(Line::from(""));
    texto.push(Line::from(
        "  Presione ENTER para inspeccionar bloques de datos.",
    ));
    texto.push(Line::from(
        "  Presione 'x' para eliminar. 'c' para limpiar finalizados.",
    ));

    let parrafo = Paragraph::new(texto).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" RESUMEN_DATOS "),
    );
    marco.render_widget(parrafo, area);
}

fn renderizar_detalle_trabajo_enmarcado(
    marco: &mut Frame,
    aplicacion: &AppState,
    trabajo: &crate::domain::Job,
    area: Rect,
) {
    let mut items_bloques = Vec::new();

    for pagina in &trabajo.document.pages {
        items_bloques.push(ListItem::new(Line::from(Span::styled(
            format!("--- PAGINA {} ---", pagina.number),
            Style::default().fg(Color::Yellow),
        ))));
        for bloque in &pagina.blocks {
            let contenido = if bloque.content.len() > 100 {
                format!("{}...", &bloque.content[..97])
            } else {
                bloque.content.clone()
            };
            items_bloques.push(ListItem::new(format!(
                "[{:?}] {}",
                bloque.block_type, contenido
            )));
        }
    }

    if items_bloques.is_empty() {
        if trabajo.status == JobStatus::Failed {
            let msg = trabajo
                .error_message
                .as_deref()
                .unwrap_or("Error desconocido");
            let parrafo = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    " JOB FALLIDO",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(Color::Red),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).title(" ERROR "));
            marco.render_widget(parrafo, area);
            return;
        } else if trabajo.status == JobStatus::Processing {
            let porcentaje = aplicacion.progreso_trabajo(&trabajo.id).unwrap_or(0.0);
            let fase = aplicacion
                .fase_trabajo(&trabajo.id)
                .unwrap_or("Procesando...");

            let indicador = ratatui::widgets::Gauge::default()
                .block(Block::default().borders(Borders::ALL).title(" PROCESANDO "))
                .gauge_style(Style::default().fg(Color::Cyan))
                .percent((porcentaje * 100.0) as u16)
                .label(fase);
            marco.render_widget(indicador, area);
            return;
        } else {
            items_bloques.push(ListItem::new("Sin contenido extraido"));
        }
    }

    let saltar = aplicacion.scroll_detalle as usize;
    let items_visibles: Vec<ListItem> = items_bloques.into_iter().skip(saltar).collect();

    let lista =
        List::new(items_visibles).block(Block::default().borders(Borders::ALL).title(format!(
            " BLOQUES (ID: {}) [t:TXT l:LaTeX p:PDF J:JSON q:Volver]",
            &trabajo.id[..8]
        )));
    marco.render_widget(lista, area);
}

/// Pantalla de carga de modelos
fn renderizar_inicializacion(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let area_centrada = rectangulo_centrado(60, 40, area);
    let secciones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area_centrada);

    let mensaje =
        Paragraph::new("Descargando e inicializando modelos ONNX...").alignment(Alignment::Center);
    marco.render_widget(mensaje, secciones[0]);

    let segundos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let glifos_carga = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner = glifos_carga[(segundos % 10) as usize];

    let porcentaje = (aplicacion.progreso_carga_motor() * 100.0) as u16;

    let etiqueta = if aplicacion.bytes_carga_total() > 0 {
        format!(
            "{} — {:.0}/{:.0} MB",
            aplicacion.fase_carga_motor(),
            aplicacion.bytes_carga_actual() as f64 / 1_048_576.0,
            aplicacion.bytes_carga_total() as f64 / 1_048_576.0,
        )
    } else {
        aplicacion.fase_carga_motor().to_string()
    };

    let indicador = ratatui::widgets::Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} CARGANDO ", spinner)),
        )
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(porcentaje)
        .label(etiqueta.as_str());

    marco.render_widget(indicador, secciones[1]);

    if !aplicacion.gpu_info().is_empty() {
        let (color, prefijo) = if aplicacion.gpu_info().contains("activa") {
            (Color::Green, "⚡ ")
        } else {
            (Color::DarkGray, "— ")
        };
        let linea_gpu = Paragraph::new(Line::from(vec![
            Span::styled(prefijo, Style::default().fg(color)),
            Span::styled(aplicacion.gpu_info(), Style::default().fg(color)),
        ]))
        .alignment(Alignment::Center);
        marco.render_widget(linea_gpu, secciones[2]);
    }
}

/// Configuración / Ajustes
fn renderizar_configuracion(marco: &mut Frame, aplicacion: &AppState, area: Rect) {
    let perfil_actual = aplicacion.perfil;
    let idioma_actual = &aplicacion.idioma.primary;

    let texto = vec![
        Line::from(""),
        Line::from(Span::styled(
            " PERFIL_PROCESAMIENTO ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw(if perfil_actual == ProcessingProfile::Fast {
                " › "
            } else {
                "   "
            }),
            Span::styled("[1] ", Style::default().fg(Color::Yellow)),
            Span::raw("RAPIDO      - Prioridad velocidad"),
        ]),
        Line::from(vec![
            Span::raw(if perfil_actual == ProcessingProfile::Balanced {
                " › "
            } else {
                "   "
            }),
            Span::styled("[2] ", Style::default().fg(Color::Yellow)),
            Span::raw("EQUILIBRADO - Por defecto"),
        ]),
        Line::from(vec![
            Span::raw(if perfil_actual == ProcessingProfile::Accurate {
                " › "
            } else {
                "   "
            }),
            Span::styled("[3] ", Style::default().fg(Color::Yellow)),
            Span::raw("PRECISO     - Prioridad exactitud"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " IDIOMA_OCR ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw(if idioma_actual == "spa" {
                " › "
            } else {
                "   "
            }),
            Span::styled("[4] ", Style::default().fg(Color::Yellow)),
            Span::raw("ESPANOL (spa)"),
        ]),
        Line::from(vec![
            Span::raw(if idioma_actual == "eng" {
                " › "
            } else {
                "   "
            }),
            Span::styled("[5] ", Style::default().fg(Color::Yellow)),
            Span::raw("INGLES  (eng)"),
        ]),
    ];

    let parrafo_config = Paragraph::new(texto).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Thick)
            .title(" AJUSTES "),
    );
    marco.render_widget(parrafo_config, area);
}

/// Diálogo de entrada de archivo
fn renderizar_dialogo_entrada(marco: &mut Frame, aplicacion: &AppState) {
    let area_centrada = rectangulo_centrado(60, 20, marco.area());

    let lineas = vec![
        Line::from(""),
        Line::from(Span::styled(
            " RUTA_ARCHIVO: ",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(" > {}_", aplicacion.buffer_entrada)),
        Line::from(""),
        Line::from(Span::styled(
            " FORMATOS: PDF, PNG, JPEG, TIFF",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ];

    let entrada = Paragraph::new(lineas).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Thick)
            .title(" AGREGAR_ARCHIVO "),
    );

    marco.render_widget(Clear, area_centrada);
    marco.render_widget(entrada, area_centrada);
}

/// Pantalla de ayuda con todos los atajos de teclado.
fn renderizar_ayuda(marco: &mut Frame, area: Rect) {
    let texto = vec![
        Line::from(""),
        Line::from(Span::styled(
            " NAVEGACION GENERAL ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ?         ", Style::default().fg(Color::Yellow)),
            Span::raw("Abrir esta pantalla de ayuda"),
        ]),
        Line::from(vec![
            Span::styled("  q / Esc   ", Style::default().fg(Color::Yellow)),
            Span::raw("Volver / Salir de la aplicacion"),
        ]),
        Line::from(vec![
            Span::styled("  s         ", Style::default().fg(Color::Yellow)),
            Span::raw("Ir a Ajustes"),
        ]),
        Line::from(vec![
            Span::styled("  j / Abajo ", Style::default().fg(Color::Yellow)),
            Span::raw("Seleccionar trabajo siguiente"),
        ]),
        Line::from(vec![
            Span::styled("  k / Arriba", Style::default().fg(Color::Yellow)),
            Span::raw("Seleccionar trabajo anterior"),
        ]),
        Line::from(vec![
            Span::styled("  Enter     ", Style::default().fg(Color::Yellow)),
            Span::raw("Ver detalle del trabajo seleccionado"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " GESTION DE TRABAJOS ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  n         ", Style::default().fg(Color::Yellow)),
            Span::raw("Agregar nuevo archivo (abre dialogo de ruta)"),
        ]),
        Line::from(vec![
            Span::styled("  z         ", Style::default().fg(Color::Yellow)),
            Span::raw("Cancelar el trabajo seleccionado (si esta en progreso)"),
        ]),
        Line::from(vec![
            Span::styled("  x         ", Style::default().fg(Color::Yellow)),
            Span::raw("Eliminar el trabajo seleccionado"),
        ]),
        Line::from(vec![
            Span::styled("  c         ", Style::default().fg(Color::Yellow)),
            Span::raw("Limpiar todos los trabajos finalizados/cancelados"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " EXPORTACION (en detalle de trabajo) ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  t         ", Style::default().fg(Color::Yellow)),
            Span::raw("Exportar como TXT (.txt)"),
        ]),
        Line::from(vec![
            Span::styled("  l         ", Style::default().fg(Color::Yellow)),
            Span::raw("Exportar como LaTeX (.tex)"),
        ]),
        Line::from(vec![
            Span::styled("  J         ", Style::default().fg(Color::Yellow)),
            Span::raw("Exportar como JSON (.json)"),
        ]),
        Line::from(vec![
            Span::styled("  p         ", Style::default().fg(Color::Yellow)),
            Span::raw("Exportar como PDF reconstruido (.pdf)"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " AJUSTES ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  1         ", Style::default().fg(Color::Yellow)),
            Span::raw("Perfil RAPIDO"),
        ]),
        Line::from(vec![
            Span::styled("  2         ", Style::default().fg(Color::Yellow)),
            Span::raw("Perfil EQUILIBRADO (por defecto)"),
        ]),
        Line::from(vec![
            Span::styled("  3         ", Style::default().fg(Color::Yellow)),
            Span::raw("Perfil PRECISO"),
        ]),
        Line::from(vec![
            Span::styled("  4         ", Style::default().fg(Color::Yellow)),
            Span::raw("Idioma Espanol (spa)"),
        ]),
        Line::from(vec![
            Span::styled("  5         ", Style::default().fg(Color::Yellow)),
            Span::raw("Idioma Ingles (eng)"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " DIAGNOSTICO ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Log: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "~/.local/share/ocrfast/ocrfast.log",
                Style::default().fg(Color::Gray),
            ),
        ]),
    ];

    let parrafo = Paragraph::new(texto).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Thick)
            .title(" AYUDA — ATAJOS DE TECLADO "),
    );
    marco.render_widget(parrafo, area);
}

/// Popup de seleccion de formato de salida.
fn renderizar_popup_formato(marco: &mut Frame, aplicacion: &AppState) {
    let area = rectangulo_centrado(40, 40, marco.area());

    let items: Vec<ListItem> = OutputFormat::OPCIONES
        .iter()
        .enumerate()
        .map(|(i, fmt)| {
            let prefijo = if i == aplicacion.indice_formato {
                " ▶ "
            } else {
                "   "
            };
            let etiqueta = format!("{}{} (.{})", prefijo, fmt.nombre(), fmt.extension());
            let estilo = if i == aplicacion.indice_formato {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Line::from(Span::styled(etiqueta, estilo)))
        })
        .collect();

    let lista = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Thick)
            .title(" Formato de salida ")
            .title_bottom(Line::from(Span::styled(
                " ↑↓ navegar  Enter confirmar  Esc cancelar ",
                Style::default().fg(Color::DarkGray),
            ))),
    );

    marco.render_widget(Clear, area);
    marco.render_widget(lista, area);
}

fn rectangulo_centrado(porcentaje_x: u16, porcentaje_y: u16, area: Rect) -> Rect {
    let layout_vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - porcentaje_y) / 2),
            Constraint::Percentage(porcentaje_y),
            Constraint::Percentage((100 - porcentaje_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - porcentaje_x) / 2),
            Constraint::Percentage(porcentaje_x),
            Constraint::Percentage((100 - porcentaje_x) / 2),
        ])
        .split(layout_vertical[1])[1]
}

fn obtener_etiqueta_y_color_estado(estado: JobStatus) -> (&'static str, Color) {
    match estado {
        JobStatus::Completed => ("[+]", Color::Green),
        JobStatus::Processing => ("[*]", Color::Yellow),
        JobStatus::Queued => ("[ ]", Color::Reset),
        JobStatus::Failed => ("[-]", Color::Red),
        JobStatus::Cancelled => ("[x]", Color::DarkGray),
    }
}
