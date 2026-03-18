# Tests

La suite de `tests/` está separada por objetivo operativo, no por capa.

## Suites

- `unit_tests.rs`: dominio, exporters, postprocessors, preprocessors, layout
  heurístico y cancelación cooperativa.
- `pipeline_integration_tests.rs`: contrato del pipeline con stubs y emisión de
  eventos.
- `document_parser_tests.rs`: parser de imágenes, errores y TIFF multipágina.
- `preprocessing_tests.rs`: formas de tensor y propiedades geométricas del
  preprocesamiento ONNX.
- `onnx_integration_tests.rs`: smoke tests del stack ONNX real. Los casos caros
  están marcados como `ignored`.
- `real_document_tests.rs`: documentos públicos reales. Requiere la feature
  `ci_real_docs` y artefactos ONNX disponibles.

## Ejecución

Suite normal:

```bash
cargo test
```

Casos ONNX ignorados:

```bash
cargo test --test onnx_integration_tests -- --ignored
```

Documentos reales:

```bash
cargo test --features ci_real_docs --test real_document_tests -- --ignored
```

## Criterio

Las pruebas rápidas deben correr sin descargas de modelos ni dependencias
externas adicionales. Los casos con artefactos pesados o documentos reales se
mantienen opt-in para no degradar el feedback loop de desarrollo.
