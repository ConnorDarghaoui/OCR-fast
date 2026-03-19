# Changelog

## Unreleased

### Added

- `BlockAutomata` como capa determinista de resolución por bloque.
- `PageComposer` como composición única por página para PDF, LaTeX, TXT y JSON.
- documentación técnica en `docs/architecture/block-automata.md` y
  `docs/architecture/page-composer.md`.

### Modified

- `HighFidelityBlueprintBuilder` ahora delega en `PageComposer`.
- `LayoutGuidedDocumentAssembler` pasa a ser un adaptador de compatibilidad
  sobre la política del compositor.
- el runtime principal deja de inyectar el ensamblador histórico en la ruta de
  procesamiento por defecto.

## 0.1.0 (2026-03-18)


### Bug Fixes

* **ci:** unify automated release flow ([c62407c](https://github.com/ConnorDarghaoui/OCR-fast/commit/c62407c925359d96f5fb1f012eafbb59764583f2))
