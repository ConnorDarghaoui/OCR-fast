# Fixtures reales locales

Esta carpeta define el layout esperado para corpus reales pesados.

Los archivos reales no se versionan. Deben ir en:

```text
tests/fixtures/real/local/
```

## Baldor fotografiado por páginas

Caso inicial recomendado:

```text
tests/fixtures/real/local/
├── manifest.json
└── baldor-pages/
    ├── 0001.jpg
    ├── 0002.jpg
    ├── 0003.jpg
    └── ...
```

## Orden

Nombra las páginas con padding fijo:

- `0001.jpg`
- `0002.jpg`
- `0003.jpg`

Eso permite que el runner preserve el orden del libro sin heurísticas extra.

## Captura sugerida

Para el caso Baldor o libros similares fotografiados:

- usa la misma orientación en todas las páginas
- no recortes unas páginas sí y otras no
- evita HDR o filtros automáticos del teléfono
- conserva también páginas difíciles: sombras, perspectiva, fórmulas, tablas

El corpus tiene que representar el problema real, no una versión ya "limpia".

## Manifest

Hay un ejemplo rastreado en:

```text
tests/fixtures/real/manifest.example.json
```

Cópielo como:

```text
tests/fixtures/real/local/manifest.json
```

y cambie las rutas al material real local.
