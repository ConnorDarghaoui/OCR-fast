# Refactor Plan: Collapse Pipeline Ownership Around Engine + Block Automata

## Branch

- Working branch: `Boniche/refactor-collapse-pipeline-ownership`
- Base: `Boniche/refactor-page-composer-block-automata`

## Motivation

The current system already moved a large part of composition into `PageComposer`
and block-level fallback into `BlockAutomata`, but responsibility is still split
across too many layers:

- `OnnxOcrEngine` already orchestrates orientation, layout, text OCR, table
  extraction, and embedded image extraction.
- `OcrPipeline` still models `layout`, `ocr`, `table`, `assemble`, and
  `blueprint` as separate global phases.
- `PageComposer` decides page mode, reading order, and visual/documental policy.
- Exporters still branch on `processing_mode` and partially re-own rendering
  policy.
- `refinement.rs` mixes useful page preprocessing with more invasive OCR retry
  logic.
- Historical compatibility adapters used to exist around composition, but the
  target state is a single concrete path through `PageComposer`.

This split makes the system harder to reason about than necessary. The same
document can have geometry, ordering, or rendering policy touched in more than
one place.

The goal of this refactor is not a big-bang rewrite. The goal is to converge on
one ownership model:

1. `OnnxOcrEngine` owns model orchestration and raw block extraction.
2. `BlockAutomata` owns per-block acceptance vs fallback.
3. `PageComposer` owns page-level policy and composition.
4. Exporters render; they do not reinterpret document policy.

## Product Invariants

These invariants must hold during and after the refactor:

1. The product outputs stay: `PDF`, `LaTeX`, `TXT`, `JSON`.
2. `PDF` remains the primary output for faithful reconstruction.
3. `LaTeX` remains the technical/editable output.
4. `TXT` remains the robust plain-text extraction output.
5. `JSON` remains the technical/debug output.
6. `DocLayout-YOLO` stays the primary geometric source of truth.
7. Fallback to raster remains guaranteed at block level.
8. No change in TUI behavior is allowed unless the simplification strictly
   removes dead choices or dead dependencies.

## What We Are Simplifying

### Current duplication

1. Geometry ownership
   - `OnnxOcrEngine` detects blocks.
   - `PageComposer` reorders blocks.
   - Exporters branch on `processing_mode`.

2. Pipeline phase ownership
   - The engine already does integrated OCR work.
   - The pipeline still exposes sub-phases as if they were independently owned.

3. Preprocessing ownership
   - `PreprocessorPort` already exists.
   - raster cleanup no longer lives in `refinement`; it belongs to `PreprocessorPort`.

4. Public API ownership
   - `PageComposer` is the concrete page-composition API.
   - The pipeline no longer exposes parallel builder/assembler hooks.

### Target ownership

After the refactor:

1. `OnnxOcrEngine` owns:
   - orientation correction
   - layout detection
   - text detection and recognition
   - table analysis
   - embedded image extraction

2. `BlockAutomata` owns:
   - text acceptance vs raster fallback
   - table acceptance vs raster fallback
   - image preservation
   - block-level confidence terminal decision

3. `PageComposer` owns:
   - per-page `processing_mode`
   - reading order policy
   - header/footer hints
   - projection to `DocumentBlueprint`

4. Exporters own:
   - format-specific rendering only

## Non-Goals

This branch will not:

- replace the ONNX model stack
- introduce LLMs
- redesign the TUI
- change storage format in `JobStore`
- add new output formats
- solve model checksum verification
- fix GitHub Actions runtime warnings

Those items stay out of scope on purpose.

## End-State Architecture

### Runtime flow

The target logical flow is:

1. `Parse`
2. `Raster` (optional page-level cleanup through `PreprocessorPort`)
3. `OcrEngine` integrated processing
4. `BlockAutomata` resolution
5. `PageComposer` composition
6. `Render`

### Ownership rule

Every major decision must have one owner:

- model orchestration -> `OnnxOcrEngine`
- block fallback -> `BlockAutomata`
- page policy -> `PageComposer`
- rendering -> exporters

No later layer should reinterpret a decision that an earlier owner already made,
unless the later layer is the explicit owner for that category.

## Detailed Commit Plan

This branch should be implemented as a sequence of small commits, even if the
work eventually lands in one PR. Each commit below is intended to be cleanly
reviewable and reversible.

### Commit 1: Document the ownership model

#### Goal

Freeze the target architecture before code movement starts.

#### Files

- `docs/architecture/block-automata.md`
- `docs/architecture/page-composer.md`
- `docs/architecture/pipeline-ownership-refactor-plan.md`

#### Changes

- Update `block-automata.md` to explicitly state that it is not responsible for
  page ordering or rendering.
- Update `page-composer.md` to explicitly state it is the single owner of page
  mode and ordering policy.
- Keep this plan file as the implementation contract.

#### Acceptance criteria

- The docs explain ownership without referencing removed architecture.
- There is no ambiguity about which layer owns which decision.

### Commit 2: Make `PageComposer` the only composition entrypoint

#### Goal

Stop direct composition policy from leaking elsewhere.

#### Files

- `src/infrastructure/page_composer/mod.rs`
- `src/infrastructure/exporters/mod.rs`
- Ensure exporter helper code only gets composed pages from `PageComposer`.
- Remove duplicated composition wrappers once `PageComposer` becomes the only
  caller-facing composition path.

#### Acceptance criteria

- There is one real composition implementation in the codebase.
- No extra wrapper layer remains between callers and `PageComposer`.

#### Regression checks

- `document_blueprint_tests`
- `unit_tests` for PDF and LaTeX

### Commit 3: Collapse exporter policy so exporters render more and decide less

#### Goal

Reduce policy branching inside exporters.

#### Files

- `src/infrastructure/exporters/mod.rs`
- `src/domain/document_blueprint.rs`

#### Changes

- Move as much policy as possible into `PageComposer` outputs.
- Keep exporters format-aware, but avoid duplicating page-policy inference.
- Prefer consuming:
  - `page.processing_mode`
  - `element.preserve_positioning`
  - `fallback_used`
  - structural hints
  instead of re-deriving policy ad hoc.

#### Acceptance criteria

- Exporters do not infer visual/document mode themselves beyond obeying the
  composed model.
- Adding a new page policy should mostly change `PageComposer`, not every
  exporter.

#### Regression checks

- PDF visual preservation tests
- LaTeX visual/document tests
- TXT ordering tests

### Commit 4: Split refinement into preprocessing vs advanced retry

#### Goal

Stop treating image cleanup and OCR retry as the same subsystem.

#### Files

- `src/application/pipeline/refinement.rs`
- `src/application/pipeline/mod.rs`
- tests under `tests/pipeline_integration_tests.rs`

#### Changes

- Keep only page-level image transformations in the main path:
  - `Deskew`
  - `Denoise`
- Mark `ConfidenceBoostPass` as advanced/optional or remove it from the default
  runtime path.
- If kept, clearly isolate it as an opt-in recovery tool instead of part of the
  standard pipeline contract.

#### Acceptance criteria

- The default runtime path does not perform global OCR retry.
- Page preprocessing has one obvious ownership path.

#### Regression checks

- startup/runtime path still works
- current tests still pass
- if `ConfidenceBoostPass` remains, it is not enabled by default

### Commit 5: Simplify pipeline stages around the integrated ONNX engine

#### Goal

Align pipeline modeling with the reality that `OnnxOcrEngine` is already an
integrated orchestrator.

#### Files

- `src/application/pipeline/mod.rs`
- `src/infrastructure/ocr_engines/onnx/engine.rs`
- `src/interfaces/ports.rs`
- `tests/pipeline_integration_tests.rs`

#### Changes

- Keep external pipeline phases for user-visible progress if needed, but reduce
  internal ownership ambiguity.
- Make it explicit that:
  - `LayoutEnginePort` is optional or legacy
  - integrated ONNX path is the canonical path
- Review whether any explicit fallback layout hook still belongs in the default
  runtime path.

#### Acceptance criteria

- The pipeline code reads as orchestration of coarse stages, not as a second
  owner of submodel choreography.
- The default runtime path clearly reflects the integrated ONNX engine as the
  canonical path.

#### Regression checks

- TUI runtime boot still works
- parser + OCR path still passes integration tests

### Commit 6: Remove legacy composition ports from the public API

#### Goal

Delete transitional composition hooks once runtime and tests stop using them.

#### Files

- `src/interfaces/ports.rs`
- `README.md`

#### Changes

- Remove `DocumentBlueprintBuilderPort`.
- Remove `DocumentAssemblerPort`.
- Remove the corresponding pipeline builder hooks.
- Move any remaining tests to `PageComposer` directly.

#### Acceptance criteria

- A new contributor sees a single public composition path.
- The pipeline API no longer suggests multiple owners for page policy.

### Commit 7: Remove `LayoutEngineFactoryPort` and keep `XyCut` as explicit fallback

#### Goal

Collapse the dead factory layer now that runtime wiring no longer depends on it.

#### Files

- `src/interfaces/ports.rs`
- `src/infrastructure/layout_engines/mod.rs`

#### Changes

- Remove `LayoutEngineFactoryPort`.
- Remove `DefaultLayoutEngineFactory`.
- Keep `LayoutEnginePort` plus `XyCutLayoutEngine` only for direct use and
  engine-level tests, not as a pipeline hook.

#### Acceptance criteria

- The factory concept disappears from the public architecture.
- Fallback layout remains available only when a caller explicitly injects it.

### Commit 8: Remove dead wiring from runtime setup

#### Goal

Ensure runtime construction only wires the canonical path.

#### Files

- `src/application/tui/job_runtime.rs`
- `src/application/tui/app_state.rs`

#### Changes

- Review constructor dependencies and remove no-longer-used injections.
- Minimize the number of moving parts required to run one job.

#### Acceptance criteria

- Runtime setup is shorter and easier to read.
- No dead dependency is still passed through the TUI if it no longer matters.

### Commit 9: Add architecture-level regression tests

#### Goal

Lock in the simplified ownership model so it does not regress.

#### Files

- `tests/pipeline_integration_tests.rs`
- `tests/document_blueprint_tests.rs`
- `tests/unit_tests.rs`
- optionally a new `tests/architecture_regression_tests.rs`

#### Changes

Add tests that assert:

1. `PageComposer` is the effective owner of page mode.
2. Low-confidence text becomes raster at block level.
3. Exporters do not reorder visual pages independently.
4. Compatibility wrappers delegate and do not add custom behavior.
5. The default runtime path does not inject legacy assembler behavior.

#### Acceptance criteria

- The tests fail if ownership drifts back into multiple layers.

### Commit 10: Final cleanup and docs sync

#### Goal

Make the simplification visible and maintainable.

#### Files

- `README.md`
- `CHANGELOG.md`
- docs files under `docs/architecture/`

#### Changes

- Update the architecture summary in the README.
- Document the simplified flow.
- Note what remained as compatibility and what became canonical.

#### Acceptance criteria

- The docs match the code after the refactor.

## File-by-File Impact Map

### Likely to change heavily

- `src/application/pipeline/mod.rs`
- `src/application/pipeline/refinement.rs`
- `src/infrastructure/exporters/mod.rs`
- `src/infrastructure/page_composer/mod.rs`
- `src/interfaces/ports.rs`

### Likely to change lightly

- `src/infrastructure/document_blueprints/mod.rs`
- `src/infrastructure/document_assemblers/mod.rs`
- `src/application/tui/job_runtime.rs`
- `src/application/tui/app_state.rs`

### Likely to stay mostly intact

- `src/infrastructure/ocr_engines/onnx/engine.rs`
- `src/infrastructure/ocr_engines/onnx/layout.rs`
- `src/infrastructure/ocr_engines/onnx/text_detection.rs`
- `src/infrastructure/ocr_engines/onnx/text_recognition.rs`
- `src/infrastructure/ocr_engines/onnx/table_analyzer.rs`
- parsers
- job store
- TUI screens

## Test Strategy

### Mandatory commands

- `cargo check`
- `cargo test --locked`

### Mandatory runtime smoke test

- `cargo run`
- verify TUI boots
- verify model bootstrap still initializes

### Regression test matrix

1. Visual screenshot-like page
   - must preserve visual ordering
   - low-confidence text must rasterize safely

2. Documental page
   - must keep text/table structure
   - must preserve page-level mode decisions

3. Hybrid document
   - visual page and documental page must coexist without exporter confusion

4. Table-heavy page
   - table strategy must still resolve to `Table` when confidence is adequate

5. Low-confidence OCR page
   - block fallback must remain localized

## Risk Register

### Risk 1: Removing too much compatibility too early

Mitigation:

- demote compatibility first
- delete only when no callers remain

### Risk 2: Exporters become under-specified

Mitigation:

- do not remove page-level hints until PDF and LaTeX clearly render from the
  composed model

### Risk 3: Breaking tests that intentionally exercise legacy ports

Mitigation:

- update tests in the same commit that changes canonical ownership
- keep wrappers alive until the suite no longer requires them

### Risk 4: Accidentally making `OnnxOcrEngine` too magical

Mitigation:

- document its ownership clearly
- keep `PageComposer` and exporters separate so the engine is not also made
  responsible for page policy or rendering

## Suggested Merge Strategy

This work should stay on one branch but be implemented as a stacked sequence of
small commits. The PR should be reviewed commit-by-commit, not as one giant diff.

Recommended commit grouping:

1. docs + ownership comments
2. `PageComposer` consolidation
3. exporter simplification
4. refinement split
5. pipeline simplification
6. legacy port demotion
7. runtime cleanup
8. tests + docs sync

## Definition of Done

The branch is complete when all of the following are true:

1. There is one canonical composition implementation: `PageComposer`.
2. Exporters consume composed policy instead of inferring it again.
3. The default runtime path does not rely on legacy assembler behavior.
4. The default path does not perform global OCR retry.
5. Legacy ports are clearly marked as compatibility or removed from default
   wiring.
6. `cargo check`, `cargo test --locked`, and a TUI smoke run all pass.
7. Documentation matches the simplified architecture.
