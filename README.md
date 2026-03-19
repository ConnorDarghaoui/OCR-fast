# OCRFast

OCR local-first en Rust con interfaz TUI para reconstrucción facsimilar y
extracción OCR. El producto se centra en `PDF`, `LaTeX`, `TXT` y `JSON`
técnico, con una arquitectura que usa layout como verdad geométrica y una capa
de composición por página para evitar reinterpretaciones duplicadas.

## Qué hace hoy

- Procesa PDF e imágenes raster (`png`, `jpg`, `jpeg`, `tiff`, `tif`, `bmp`, `webp`).
- Arranca la TUI de inmediato con un motor stub y carga ONNX en background.
- Descarga modelos ONNX automáticamente cuando no existen en la máquina.
- Usa `pdfium` para rasterizar PDF sin depender de una instalación manual.
- Persiste trabajos en disco para recuperar estado entre sesiones.
- Expone un pipeline desacoplado por puertos para parser, layout, OCR,
  preprocesamiento, composición por página, exportación y storage.
- Resuelve bloques con un autómata determinista y compone páginas en un modelo
  visual único antes de renderizar PDF/LaTeX.

## Modelo de ejecución real

El comportamiento actual no es “100% offline desde cero”.

- `cargo build` puede descargar dependencias nativas, incluido `pdfium`.
- `ort` usa `download-binaries`, así que la primera compilación también puede
  bajar binarios de ONNX Runtime.
- El primer arranque con motor real puede descargar modelos a la ruta local de
  datos del usuario.
- Si ejecutas con `--stub`, la aplicación no intenta cargar ONNX.

La aplicación sigue siendo local-first en el sentido operativo: una vez que los
artefactos están presentes, el procesamiento ocurre en la máquina del usuario.

## Rutas locales

OCRFast usa `dirs::data_local_dir()/ocrfast/` como base de runtime:

- `jobs.json`: snapshots persistidos de trabajos.
- `models/`: artefactos ONNX descargados.
- `ocrfast.log`: log estructurado de la aplicación.

En Linux normalmente eso resuelve a `~/.local/share/ocrfast/`.

## Requisitos

- Rust estable reciente.
- Linux, macOS o Windows.
- Conectividad de red solo para la primera descarga de dependencias/modelos si
  aún no existen localmente.

## Inicio rápido

```bash
git clone https://github.com/ConnorDarghaoui/OCR-fast.git
cd OCR-fast
cargo build --release
cargo run --release
```

Modo stub explícito:

```bash
cargo run --release -- --stub
```

Binario compilado:

```bash
./target/release/ocrfast
```

## Controles de la TUI

### Vista de trabajos

- `n`: agregar archivo.
- `j` / `k` o flechas: navegar.
- `Enter`: abrir detalle.
- `s`: abrir ajustes.
- `x`: eliminar trabajo seleccionado.
- `c`: limpiar trabajos finalizados.
- `z`: solicitar cancelación del trabajo en curso.
- `?`: abrir ayuda.
- `q`: salir.

### Vista de detalle

- `j` / `k` o flechas: scroll.
- `e`: exportar al formato seleccionado.
- `E`: exportar a JSON.
- `p`: exportar a PDF.
- `z`: cancelar si el trabajo sigue corriendo.
- `Esc` o `q`: volver a la lista.

### Ajustes

- `1`: perfil `Fast`.
- `2`: perfil `Balanced`.
- `3`: perfil `Accurate`.
- `4`: idioma primario `spa`.
- `5`: idioma primario `eng`.
- `Esc` o `q`: volver.

### Entrada de ruta

- Escribir: ruta del archivo.
- `Enter`: confirmar.
- `Backspace`: borrar.
- `Esc`: cancelar.

## Formatos soportados

### Entrada

- PDF
- PNG
- JPEG
- TIFF
- BMP
- WebP

### Salida

- TXT
- LaTeX
- PDF reconstruido o preservado según el modo de página
- JSON estructurado

## GPU y features

La base funciona en CPU. Si quieres compilar con soporte de aceleración
específico, usa las features expuestas por `ort`:

```bash
cargo build --release --features cuda
```

Features disponibles:

- `cuda`
- `tensorrt`
- `rocm`
- `coreml`

La aplicación degrada a CPU si el backend solicitado no queda operativo.

## Arquitectura

```text
src/
├── domain/
│   ├── mod.rs
│   └── errors.rs
├── interfaces/
│   ├── mod.rs
│   └── ports.rs
├── application/
│   ├── pipeline/mod.rs
│   └── tui/
│       ├── mod.rs
│       ├── app_state.rs
│       ├── events.rs
│       └── ui.rs
├── infrastructure/
│   ├── automata/
│   ├── document_parsers/
│   ├── document_blueprints/
│   ├── exporters/
│   ├── job_store/
│   ├── layout_engines/
│   ├── ocr_engines/
│   ├── page_composer/
│   ├── postprocessors/
│   └── preprocessors/
├── lib.rs
└── main.rs
```

### Responsabilidad por capa

- `domain`: entidades, enums y errores del sistema.
- `interfaces`: puertos que definen contratos estables.
- `application`: orquestación del pipeline y estado de la TUI.
- `infrastructure`: adaptadores concretos para parsing, layout, OCR,
  persistencia, composición y exportación.

## Arquitectura visual actual

- `DocLayout-YOLO` sigue siendo la verdad geométrica.
- `BlockAutomata` decide por bloque si el contenido final queda como texto,
  tabla, imagen o fallback raster.
- `PageComposer` decide el modo efectivo por página, fija el orden de lectura
  y produce un `DocumentBlueprint` estable.
- Los exportadores PDF y LaTeX consumen ese modelo compuesto en lugar de
  recalcular layout por su cuenta.

## Pruebas

La carpeta [`tests/`](/home/lucas/Documents/proyectos/utp/ocrfast/tests) ya
quedó limpia de comentarios narrativos y tiene una guía breve en
[tests/README.md](/home/lucas/Documents/proyectos/utp/ocrfast/tests/README.md).

Comandos útiles:

```bash
cargo test
```

```bash
cargo test --test onnx_integration_tests -- --ignored
```

```bash
cargo test --features ci_real_docs --test real_document_tests -- --ignored
```

## Limitaciones actuales

- El arranque inicial con OCR real depende de que puedan descargarse modelos.
- La TUI comienza con stub y luego reemplaza el backend por ONNX cuando termina
  de cargar.
- Los comentarios y rustdoc del código ya están mucho más consistentes, pero la
  documentación de API no pretende ser tutorial de uso; está orientada a diseño,
  trade-offs y mantenimiento.

## Licencia

MIT
