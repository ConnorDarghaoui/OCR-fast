# Corpus Real de Evaluación

El siguiente salto de calidad para OCRFast no es otra refactorización grande.
Es medir el sistema con documentos reales y difíciles.

## Objetivo

Construir un corpus reproducible que permita responder, con datos:

- qué tan bien se reconstruye un documento real
- cuándo el sistema cae a preservación visual
- qué tipos de bloque fuerzan fallback raster
- cuánto tiempo y memoria consume por página

## Primer caso: Baldor fotografiado por páginas

El primer caso objetivo es un libro de Baldor capturado con fotos, no con escaneo
plano. Eso es valioso porque mezcla:

- perspectiva imperfecta
- iluminación desigual
- texto denso
- fórmulas
- diagramas y tablas
- ruido de fondo y márgenes variables

Si OCRFast resiste este caso, el sistema mejora de verdad.

## Qué guardar

Para el caso `photo_book`, el material ideal es:

1. 8 a 20 páginas consecutivas.
2. Nombres ordenados de forma estable: `0001.jpg`, `0002.jpg`, etc.
3. Sin edición manual agresiva previa.
4. Si existe una referencia humana mínima, anotar:
   - si la página tiene muchas fórmulas
   - si tiene tablas
   - si la foto está torcida
   - si hay sombra o perspectiva fuerte

## Recomendaciones de captura para Baldor

Para que el corpus diga algo útil sobre OCRFast y no solo sobre una mala foto:

- usar siempre la misma distancia aproximada a la página
- evitar recortes manuales inconsistentes entre páginas
- intentar luz uniforme, aunque no sea perfecta
- no aplicar filtros automáticos del teléfono
- mantener visible la página completa, incluido margen interior si existe
- si una página sale muy torcida o con sombra fuerte, conservarla igual y anotarla

La idea no es "limpiar" el caso difícil. La idea es medirlo bien.

## Layout de archivos

El repo rastrea solo el esqueleto.
Los archivos pesados viven fuera de Git bajo:

```text
tests/fixtures/real/local/
```

Un ejemplo mínimo para Baldor:

```text
tests/fixtures/real/local/
├── manifest.json
└── baldor-pages/
    ├── 0001.jpg
    ├── 0002.jpg
    ├── 0003.jpg
    └── ...
```

## Runner

El binario local para evaluación es:

```bash
cargo run --bin corpus_benchmark -- tests/fixtures/real/local/manifest.json
```

Si no pasas parámetros, intentará usar:

```text
tests/fixtures/real/local/manifest.json
```

Y escribirá artefactos y reporte en:

```text
tests/fixtures/real/output/
```

## Qué reportar por caso

El runner genera, como mínimo:

- entradas procesadas
- páginas totales
- bloques totales
- bloques textuales, tabulares e imágenes
- bloques con fallback raster
- páginas visuales vs documentales
- tiempo total por caso
- confianza OCR promedio aproximada
- artefactos exportados

## Estrategia recomendada para el primer pase

Para Baldor fotografiado, el primer pase útil es:

- `pdf`: para ver si la salida final sigue siendo utilizable
- `json`: para inspeccionar bloques, modos de página y fallbacks
- `txt`: solo como referencia rápida, no como criterio principal

Si el caso viene muy cargado de fórmulas o diagramas, el PDF y el JSON te van a
decir más que el TXT sobre dónde está fallando el sistema.

## Criterio de éxito inicial

Para el caso Baldor, el objetivo inicial no es “perfecto”.
El objetivo inicial es:

- procesar todas las páginas sin colapsar
- obtener un PDF final utilizable
- preservar bien las zonas difíciles vía fallback
- producir TXT y JSON diagnósticos
- identificar con precisión dónde falla

Después de eso se atacan hotspots concretos.
