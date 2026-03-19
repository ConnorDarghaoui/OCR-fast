# Block Automata

## Objetivo

Reducir la complejidad del pipeline haciendo que la unidad de decisión sea el
bloque detectado por layout, no el documento completo. La geometría detectada
por YOLO se trata como verdad estable; el autómata solo decide cómo resolver el
contenido de cada bloque.

## Ownership

`BlockAutomata` es el dueño exclusivo de la decisión terminal por bloque:

- aceptar texto OCR
- aceptar una tabla estructurada
- preservar una imagen
- degradar a raster por baja confianza

Lo que **no** le pertenece:

- inferir `ProcessingMode` por página
- reordenar bloques
- decidir headers/footers
- renderizar PDF o LaTeX
- reintentar OCR global del documento

Eso evita que el autómata vuelva a crecer como un segundo pipeline oculto.

## Flujo

1. `DocLayout-YOLO` produce un conjunto de `DetectedBlock`.
2. `BlockAutomata` selecciona una estrategia según el tipo de bloque.
3. La estrategia devuelve contenido aceptado o el autómata degrada a raster.
4. El resultado queda en `ResolvedBlock`.

La salida de `ResolvedBlock` debe considerarse una frontera estable: las capas
posteriores pueden renderizarla o componerla, pero no deben reinterpretar su
política de fallback.

## Estrategias actuales

- `TextBlockStrategy`: acepta texto OCR cuando la confianza supera el umbral.
- `TableBlockStrategy`: acepta `TableStructure` o degrada a raster si la tabla
  no es confiable.
- `ImageBlockStrategy`: preserva el recorte original como imagen.

## Garantías

- Cada bloque tiene una sola decisión terminal.
- No hay reintentos ni loops dentro del autómata.
- Siempre existe un fallback seguro al recorte raster original.

## Beneficio arquitectónico

El autómata elimina decisiones duplicadas en exportadores y reduce la
dependencia de heurísticas globales para casos donde la geometría ya está
resuelta por layout.

En términos de ownership:

- `OnnxOcrEngine` extrae señal de modelos
- `BlockAutomata` decide aceptación vs fallback
- `PageComposer` decide política de página
- exportadores renderizan

Si una capa posterior necesita "pensar de nuevo" si un bloque era texto,
imagen o raster, el ownership volvió a romperse.
