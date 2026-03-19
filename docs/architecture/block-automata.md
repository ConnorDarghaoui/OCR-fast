# Block Automata

## Objetivo

Reducir la complejidad del pipeline haciendo que la unidad de decisión sea el
bloque detectado por layout, no el documento completo. La geometría detectada
por YOLO se trata como verdad estable; el autómata solo decide cómo resolver el
contenido de cada bloque.

## Flujo

1. `DocLayout-YOLO` produce un conjunto de `DetectedBlock`.
2. `BlockAutomata` selecciona una estrategia según el tipo de bloque.
3. La estrategia devuelve contenido aceptado o el autómata degrada a raster.
4. El resultado queda en `ResolvedBlock`.

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
