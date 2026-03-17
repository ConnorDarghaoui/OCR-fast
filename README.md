# OCRfast

Sistema OCR (Reconocimiento Óptico de Caracteres) local-first implementado en Rust con interfaz TUI.

## Características

- **Terminal User Interface (TUI)**: Interfaz interactiva moderna y minimalista
  - Diseño adaptativo que respeta el tema de tu terminal
  - Soporte de mouse (scroll para navegar)
  - Badges ASCII puros (sin emojis): `[+]` `[-]` `[*]` `[ ]`
  - Rendimiento optimizado (>60 FPS)
- **Procesamiento local**: Total independencia de conexiones a Internet
- **Clean Architecture**: Arquitectura limpia que separa dominio, aplicación e infraestructura
- **Soporte de formatos**: PDF, PNG, JPEG y otros formatos de imagen
- **Motores OCR flexibles**: Compatible con Tesseract y motores ONNX
- **Perfiles de procesamiento**: Modos rápido, preciso y balanceado
- **Exportación múltiple**: Soporte para Markdown, PDF Sandwich y JSON

## Instalación

### Requisitos
- Rust 1.70 o superior
- Sistema operativo: Linux, Windows o macOS

### Compilar desde código fuente

Este flujo puede descargar dependencias nativas y modelos si no existen localmente.

```bash
# Clonar el repositorio
git clone <repo-url>
cd ocrfast

# Compilar en modo release
cargo build --release

# El binario estará en target/release/ocrfast
```

## Uso

### Ejecutar la aplicación

```bash
# Desde el directorio del proyecto
cargo run

# O ejecutar el binario directamente
./target/release/ocrfast
```

Los modelos ONNX se descargan automáticamente si no existen localmente.

### Controles de la interfaz TUI

#### Vista principal (Lista de trabajos)
- **`n`**: Agregar nuevo archivo para procesar
- **`↑`/`↓`** o **`j`/`k`**: Navegar entre trabajos
- **Mouse scroll**: Navegar con rueda del mouse (arriba/abajo)
- **`Enter`**: Ver detalles del trabajo seleccionado
- **`s`**: Abrir configuración
- **`q`**: Salir de la aplicación

#### Vista de detalles
- **`q`** o **`Esc`**: Volver a la lista de trabajos

#### Configuración
- **`1`**: Perfil Fast (rápido)
- **`2`**: Perfil Balanced (balanceado)
- **`3`**: Perfil Accurate (preciso)
- **`q`** o **`Esc`**: Volver a la lista

#### Modo de edición (al agregar archivo)
- **Escribir**: Ingresar ruta del archivo
- **`Enter`**: Procesar el archivo
- **`Esc`**: Cancelar
- **`Backspace`**: Borrar caracteres

## Arquitectura

El sistema sigue principios de Clean Architecture con las siguientes capas:

```
src/
├── domain/           # Entidades y reglas de negocio fundamentales
├── application/      # Casos de uso y coordinación de la lógica
│   ├── tui/          # Terminal User Interface
│   │   ├── mod.rs          # Inicialización y cleanup
│   │   ├── app_state.rs    # Estado reactivo de la aplicación
│   │   ├── events.rs       # Event loop y manejo de teclado
│   │   └── ui.rs           # Renderizado de widgets
│   └── use_cases/    # Implementación de casos de uso
├── interfaces/       # Definición de puertos (abstracciones)
└── infrastructure/   # Implementaciones concretas de servicios externos
    ├── ocr_engines/  # Motores OCR (Tesseract, ONNX, stubs)
    ├── document_parsers/ # Parseadores de documentos
    ├── job_store/    # Almacenamiento de trabajos
    └── exporters/    # Exportadores de resultados
```

## Desarrollo

### Ejecutar tests

```bash
cargo test
```

### Compilar en modo debug

```bash
cargo build
```

### Logs

La aplicación usa `env_logger`. Para ver logs detallados:

```bash
RUST_LOG=debug cargo run
```

## Stubs vs Implementaciones reales

Actualmente, el proyecto usa **stubs** (implementaciones simuladas) para desarrollo y testing rápido:

- `StubDocumentParser`: Simula parseo de documentos sin I/O real
- `StubOcrEngine`: Genera texto de ejemplo con confianza variable

### Migrar a implementaciones reales

Para producción, reemplaza los stubs en `src/main.rs`:

```rust
// En lugar de:
let parser = Arc::new(StubDocumentParser::new());
let ocr_engine = Arc::new(StubOcrEngine::new());

// Usa:
let parser = Arc::new(DocumentParser::new());
let ocr_engine = Arc::new(TesseractEngine::new());
```

## Roadmap

- [ ] Integración con Tesseract real (tesseract-rs)
- [ ] Integración con ONNX Runtime
- [ ] Persistencia de jobs en disco
- [ ] Exportación a Markdown/PDF
- [ ] Layout engines (XY-Cut, ONNX-based)
- [ ] Preprocesamiento de imágenes
- [ ] Tests E2E

## Tecnologías

- **Rust**: Lenguaje de programación
- **ratatui**: Framework TUI para rendering de widgets
- **crossterm**: Biblioteca cross-platform para control de terminal
- **uuid**: Generación de identificadores únicos
- **serde**: Serialización/deserialización
- **chrono**: Manejo de fechas y tiempos
- **log + env_logger**: Sistema de logging

## Licencia

MIT

## Contribuir

Las contribuciones son bienvenidas. Por favor, abre un issue primero para discutir cambios mayores.
