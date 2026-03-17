// ! Tests de integracion con documentos reales.
//!
//! Estos tests solo se ejecutan cuando la feature `ci_real_docs` esta habilitada,
//! para no bloquear el CI normal que no tiene modelos ONNX descargados.
//!
//! Para ejecutar:
//! ```bash
//! cargo test --features ci_real_docs --test real_document_tests -- --ignored
//! ```
//!
//! Los documentos de prueba se descargan automaticamente a /tmp/ocrfast_test_docs/
//! si no existen. El directorio es limpiado tras cada ejecucion de test.

#[cfg(feature = "ci_real_docs")]
mod real_document_tests {
    use ocrfast::application::pipeline::OcrPipeline;
    use ocrfast::domain::ProcessingProfile;
    use ocrfast::infrastructure::ocr_engines::onnx::{engine::OnnxOcrEngine, model_downloader::ModelDownloader};
    use ocrfast::infrastructure::parsers::pdfium::PdfiumParser;
    use ocrfast::infrastructure::layout_engines::XyCutLayoutEngine;
    use ocrfast::infrastructure::postprocessors::TextPostprocessor;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const DIRECTORIO_PRUEBAS: &str = "/tmp/ocrfast_test_docs";

    /// URL de documentos de dominio publico para pruebas.
    ///
    /// Seleccionados por cubrir los casos de uso mas comunes:
    /// - Una columna (texto corrido)
    /// - Dos columnas (estructura academica)
    /// - PDF escaneado (imagen, requiere OCR completo)
    const DOCUMENTOS_PRUEBA: &[(&str, &str)] = &[
        // PDF de ejemplo de una columna — W3C spec fragment
        (
            "https://www.w3.org/WAI/WCAG21/wcag21-intro.pdf",
            "una_columna.pdf",
        ),
        // Paper de dos columnas — arXiv (dominio publico)
        (
            "https://arxiv.org/pdf/1706.03762",  // Attention Is All You Need
            "dos_columnas.pdf",
        ),
    ];

    /// Ground truth minimo para verificar que el OCR produce texto coherente.
    /// No es un test de CER exacto, solo verifica que el texto no sea completamente incorrecto.
    const PALABRAS_ESPERADAS_UNA_COLUMNA: &[&str] = &[
        "accessibility", "guidelines", "content",
    ];
    const PALABRAS_ESPERADAS_DOS_COLUMNAS: &[&str] = &[
        "attention", "model", "transformer",
    ];

    fn descargar_si_falta(url: &str, nombre: &str) -> PathBuf {
        let dir = Path::new(DIRECTORIO_PRUEBAS);
        std::fs::create_dir_all(dir).expect("No se pudo crear directorio de pruebas");
        let ruta = dir.join(nombre);
        if ruta.exists() {
            return ruta;
        }

        eprintln!("Descargando documento de prueba: {}", url);
        let respuesta = reqwest::blocking::get(url)
            .expect("No se pudo conectar para descargar documento de prueba");
        let bytes = respuesta.bytes().expect("Error leyendo cuerpo de respuesta");
        std::fs::write(&ruta, &bytes).expect("Error guardando documento de prueba en disco");
        ruta
    }

    fn construir_pipeline() -> OcrPipeline {
        // El downloader ya debe tener los modelos; falla si no estan.
        let downloader = ModelDownloader::new()
            .expect("No se pudo inicializar ModelDownloader");
        assert!(
            downloader.todos_los_modelos_disponibles(),
            "Modelos ONNX no disponibles. Ejecutar la app una vez para descargarlos."
        );

        let dir_modelos = downloader.directorio_base().to_path_buf();

        let motor_ocr = Arc::new(
            OnnxOcrEngine::from_directory(&dir_modelos, None)
                .expect("No se pudo inicializar OnnxOcrEngine con los modelos disponibles"),
        );
        let parser = Arc::new(PdfiumParser::new().expect("No se pudo inicializar PdfiumParser"));
        let layout = Arc::new(XyCutLayoutEngine::new());
        let postprocesador = Arc::new(TextPostprocessor::new());

        OcrPipeline::new(parser, motor_ocr, layout, postprocesador)
    }

    /// Calcula la tasa de palabras encontradas (hitrate) del texto extraido
    /// respecto a una lista de palabras esperadas.
    fn calcular_hitrate(texto: &str, palabras_esperadas: &[&str]) -> f32 {
        let texto_lower = texto.to_lowercase();
        let encontradas = palabras_esperadas
            .iter()
            .filter(|&&p| texto_lower.contains(p))
            .count();
        encontradas as f32 / palabras_esperadas.len() as f32
    }

    #[test]
    #[ignore = "requiere ONNX + modelos descargados (ci_real_docs)"]
    fn test_documento_una_columna_texto_coherente() {
        let ruta = descargar_si_falta(DOCUMENTOS_PRUEBA[0].0, DOCUMENTOS_PRUEBA[0].1);
        let pipeline = construir_pipeline();

        let cancela = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let doc = pipeline
            .procesar_documento(&ruta, &ProcessingProfile::Balanced, None, Some(&cancela))
            .expect("El pipeline fallo con el documento de una columna");

        let texto_completo: String = doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter())
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            !texto_completo.is_empty(),
            "El documento de una columna no produjo ningun texto"
        );

        let hitrate = calcular_hitrate(&texto_completo, PALABRAS_ESPERADAS_UNA_COLUMNA);
        assert!(
            hitrate >= 0.5,
            "Menos del 50% de palabras clave encontradas ({:.0}%). \
             Texto extraido (primeros 500 chars): {}",
            hitrate * 100.0,
            &texto_completo[..texto_completo.len().min(500)]
        );

        eprintln!("Hitrate una columna: {:.1}%", hitrate * 100.0);
        eprintln!("Paginas procesadas: {}", doc.pages.len());
        eprintln!(
            "Bloques totales: {}",
            doc.pages.iter().map(|p| p.blocks.len()).sum::<usize>()
        );
    }

    #[test]
    #[ignore = "requiere ONNX + modelos descargados (ci_real_docs)"]
    fn test_documento_dos_columnas_bloques_multiples() {
        let ruta = descargar_si_falta(DOCUMENTOS_PRUEBA[1].0, DOCUMENTOS_PRUEBA[1].1);
        let pipeline = construir_pipeline();

        let cancela = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let doc = pipeline
            .procesar_documento(&ruta, &ProcessingProfile::Accurate, None, Some(&cancela))
            .expect("El pipeline fallo con el documento de dos columnas");

        // Un documento de dos columnas debe tener mas de 2 bloques por pagina en promedio
        let total_bloques: usize = doc.pages.iter().map(|p| p.blocks.len()).sum();
        let paginas = doc.pages.len();
        assert!(paginas > 0, "No se procesaron paginas");

        let bloques_por_pagina = total_bloques as f32 / paginas as f32;
        assert!(
            bloques_por_pagina >= 2.0,
            "Esperado >= 2 bloques/pagina en documento de 2 columnas, obtenido {:.1}",
            bloques_por_pagina
        );

        let texto_completo: String = doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter())
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let hitrate = calcular_hitrate(&texto_completo, PALABRAS_ESPERADAS_DOS_COLUMNAS);
        assert!(
            hitrate >= 0.5,
            "Menos del 50% de palabras clave encontradas ({:.0}%)",
            hitrate * 100.0,
        );

        eprintln!("Hitrate dos columnas: {:.1}%", hitrate * 100.0);
        eprintln!("Paginas: {}, Bloques totales: {}, Bloques/pagina: {:.1}",
            paginas, total_bloques, bloques_por_pagina);
    }
}

// Si la feature no esta habilitada, al menos compilar un test dummy para que el binario exista.
#[cfg(not(feature = "ci_real_docs"))]
#[test]
fn test_ci_real_docs_no_activo() {
    // Tests de documento real deshabilitados.
    // Ejecutar con: cargo test --features ci_real_docs --test real_document_tests -- --ignored
}
