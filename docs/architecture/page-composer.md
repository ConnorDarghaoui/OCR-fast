# Page Composer

## Objetivo

Unificar en una sola capa la composición visual que antes estaba repartida entre
el ensamblador final y el builder de blueprint. `PageComposer` decide el modo de
procesamiento por página, fija el orden de lectura y proyecta el modelo visual
estable usado por PDF, LaTeX, TXT y JSON.

## Ownership

`PageComposer` es la fuente canónica de política por página. Eso incluye:

- `ProcessingMode` efectivo
- orden de lectura
- preservación visual vs reconstrucción documental
- hints conservadores de `header/footer`
- proyección del modelo que consumen los renderizadores

Lo que **no** le pertenece:

- correr modelos ONNX
- hacer OCR o análisis tabular
- decidir fallback por bloque
- renderizar un formato final específico

## Responsabilidades

- inferir `ProcessingMode` por página
- respetar override manual desde metadata
- preservar orden visual en páginas facsimilares
- ordenar lectura en páginas documentales
- marcar hints conservadores de `header/footer`
- producir un `DocumentBlueprint` coherente para renderización

## Relación con el autómata

`PageComposer` no hace OCR ni layout. Recibe páginas ya segmentadas y usa
`BlockAutomata` para resolver el contenido de cada bloque antes de proyectarlo a
`ElementBlueprint`.

La relación correcta entre ambos módulos es:

- `BlockAutomata` resuelve contenido local por bloque
- `PageComposer` decide cómo conviven esos bloques dentro de una página

Si `BlockAutomata` empieza a ordenar bloques o si el exportador vuelve a inferir
el `ProcessingMode`, el ownership vuelve a quedar duplicado.

## API actual

`PageComposer` ya es la API concreta del producto para composición por página.
No existe una segunda capa de builder o assembler que vuelva a envolver esta
política.

## Beneficio arquitectónico

La salida rica deja de nacer de varias capas con heurísticas parcialmente
superpuestas. Eso reduce divergencia entre TXT, PDF y LaTeX, y hace que los
cambios de orden o de política visual se concentren en un solo punto.

La regla operacional es simple:

- la TUI expresa preferencia
- `PageComposer` decide el modo efectivo
- exportadores obedecen

No debería haber otra capa intermedia reinterpretando esa decisión.
