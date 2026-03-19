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

## [0.2.0](https://github.com/ConnorDarghaoui/OCR-fast/compare/v0.1.0...v0.2.0) (2026-03-19)


### Features

* **blueprint:** add conservative header footer hints ([bf07883](https://github.com/ConnorDarghaoui/OCR-fast/commit/bf07883000829d87fe4f8155aa806dbb57308316))
* **blueprint:** add ocr and layout confidence signals ([27d8624](https://github.com/ConnorDarghaoui/OCR-fast/commit/27d862420ba7aa4c9ac563802b02afaa4bedf4da))
* **docx:** fallback low confidence blocks to raster ([f3c9abd](https://github.com/ConnorDarghaoui/OCR-fast/commit/f3c9abdb1f654a1dbf12ffe8ccc4ff9b25859a5b))
* **export:** add docx and latex fidelity outputs ([7ecda1a](https://github.com/ConnorDarghaoui/OCR-fast/commit/7ecda1a5da292930fc30b2ebdd87196aed25a934))
* **latex:** add facsimile export mode ([fc7fe2a](https://github.com/ConnorDarghaoui/OCR-fast/commit/fc7fe2a5c5590867aec61c806421bc710182ba78))
* **latex:** add optional compiler validation ([f4eeb9e](https://github.com/ConnorDarghaoui/OCR-fast/commit/f4eeb9e46c11064b9ba85674223c6de45d4cf5cf))
* **latex:** add semantic export mode ([847b605](https://github.com/ConnorDarghaoui/OCR-fast/commit/847b6050004d9513b8e744d753dfb5b1afd6f0fa))
* **layout:** improve fidelity hints and column-aware docx ([65df884](https://github.com/ConnorDarghaoui/OCR-fast/commit/65df88493a293e4b0f527f97e1adceb7605286ed))
* **pdf:** add reconstructed facsimile export ([b02e438](https://github.com/ConnorDarghaoui/OCR-fast/commit/b02e438ee15c5045427cfb89977658699e82793a))
* **pdf:** fallback low confidence blocks to raster crops ([0973a13](https://github.com/ConnorDarghaoui/OCR-fast/commit/0973a13a469e0268b85ac9a15acddfc136977db5))
* **pipeline:** add first refinement passes ([218f6c2](https://github.com/ConnorDarghaoui/OCR-fast/commit/218f6c2213f45b261e0a90431105fa53ae2c435d))
* **pipeline:** add high fidelity document blueprint base ([167e887](https://github.com/ConnorDarghaoui/OCR-fast/commit/167e887059824f05cf7ed84577fb0a879eba8fed))
* **pipeline:** add visual preservation processing mode ([43a49a8](https://github.com/ConnorDarghaoui/OCR-fast/commit/43a49a85df0557a4b6c65c6c56ddc366adaa9d95))
* **pipeline:** add visual preservation processing mode ([8207ca2](https://github.com/ConnorDarghaoui/OCR-fast/commit/8207ca2e82471accfad92cd7ed12f070f2bdbd79))
* **pipeline:** assemble final document from layout ([12cd70a](https://github.com/ConnorDarghaoui/OCR-fast/commit/12cd70a92fbe5e925d009302203ca391d0a8da49))
* **pipeline:** assemble final document from layout ([7b7b0ed](https://github.com/ConnorDarghaoui/OCR-fast/commit/7b7b0ede7c5e6bce69f42b35b642768e07bfcbe1))
* **table:** enrich table structure metadata for exporters ([64fdedb](https://github.com/ConnorDarghaoui/OCR-fast/commit/64fdedb1adf20d575edaa205ed47f78fe7a6d031))
* **tui:** add manual processing mode selection ([cda5811](https://github.com/ConnorDarghaoui/OCR-fast/commit/cda58111cca63f97e7de7879f27d8cac55ee37d7))
* **tui:** add manual processing mode selection ([81184fa](https://github.com/ConnorDarghaoui/OCR-fast/commit/81184fa9ba735293d7bc2eebaa9d212e9ef365d3))


### Bug Fixes

* **ci:** harden github actions workflows ([3e5594f](https://github.com/ConnorDarghaoui/OCR-fast/commit/3e5594f9ad2adf37947e996a8bc16fa9001946ca))
* **ci:** harden github actions workflows ([9c5ef4d](https://github.com/ConnorDarghaoui/OCR-fast/commit/9c5ef4d67443fe85aa072e450c164e3b6bb741c4))
* **ci:** unify automated release flow ([c62407c](https://github.com/ConnorDarghaoui/OCR-fast/commit/c62407c925359d96f5fb1f012eafbb59764583f2))
* **infra:** harden job store and model downloader ([d7f21b1](https://github.com/ConnorDarghaoui/OCR-fast/commit/d7f21b1e4d5af278cef3c94abf27b43aed1a6c5c))
* **job-store:** add cross-instance file locking ([2cd9e00](https://github.com/ConnorDarghaoui/OCR-fast/commit/2cd9e0011b0128d32c7c03b9e283489c5cdfe863))
* **pdf:** compress embedded image xobjects ([7b56eb2](https://github.com/ConnorDarghaoui/OCR-fast/commit/7b56eb2c70d0abe8a7bbc24cedfb43a8be21655e))
* **pdf:** encode text with winansi bytes ([5e243b4](https://github.com/ConnorDarghaoui/OCR-fast/commit/5e243b46b6c345e5aed45fc90eec80d2df304cea))
* **pdf:** use helvetica glyph metrics for wrapping ([afe0950](https://github.com/ConnorDarghaoui/OCR-fast/commit/afe0950a3f6502ebcc41e2b5c2a5e2b8a5b5aa93))
* **pipeline:** harden refinement automata execution ([2cb8044](https://github.com/ConnorDarghaoui/OCR-fast/commit/2cb80445ec0a97015e7520f93c83b4a91730dddb))
* **storage:** harden file job store persistence ([9810ce9](https://github.com/ConnorDarghaoui/OCR-fast/commit/9810ce92881c5d1ab7445cdc2efb389a996897f5))
* **storage:** harden file job store persistence ([9b091f7](https://github.com/ConnorDarghaoui/OCR-fast/commit/9b091f73596d609817535f0abb2ceb98e1de0bb1))
* **tests:** add layout confidence to txt order fixture ([41c017d](https://github.com/ConnorDarghaoui/OCR-fast/commit/41c017dd20014794dd63d4dd654604d6f0f932d7))

## 0.1.0 (2026-03-18)


### Bug Fixes

* **ci:** unify automated release flow ([c62407c](https://github.com/ConnorDarghaoui/OCR-fast/commit/c62407c925359d96f5fb1f012eafbb59764583f2))
