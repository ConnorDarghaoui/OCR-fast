# Page Composer

## Objetivo

Unificar en una sola capa la composición visual que antes estaba repartida entre
el ensamblador final y el builder de blueprint. `PageComposer` decide el modo de
procesamiento por página, fija el orden de lectura y proyecta el modelo visual
estable usado por PDF, LaTeX, TXT y JSON.

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

## Compatibilidad

`HighFidelityBlueprintBuilder` y `LayoutGuidedDocumentAssembler` siguen
existiendo como adaptadores finos para no romper la API actual, pero la lógica
real ya vive en `PageComposer`.

## Beneficio arquitectónico

La salida rica deja de nacer de dos módulos distintos con heurísticas
parcialmente superpuestas. Eso reduce divergencia entre TXT, PDF y LaTeX, y hace
que los cambios de orden o de política visual se concentren en un solo punto.
