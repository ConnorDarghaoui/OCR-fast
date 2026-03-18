#[cfg(test)]
mod tests {
    use image::{DynamicImage, RgbImage};
    use ocrfast::infrastructure::ocr_engines::onnx::ModelDownloader;

    /// Genera una imagen sintetica 800x600 con rectangulo negro
    /// simulando un bloque de texto.
    fn generar_imagen_con_texto_simulado() -> DynamicImage {
        let mut img = RgbImage::new(800, 600);

        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([255, 255, 255]);
        }

        for y in 50..80 {
            for x in 100..500 {
                img.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }

        for y in 120..250 {
            for x in 50..750 {
                if y % 15 < 8 {
                    img.put_pixel(x, y, image::Rgb([30, 30, 30]));
                }
            }
        }

        for y in 300..500 {
            for x in 200..600 {
                img.put_pixel(x, y, image::Rgb([128, 128, 128]));
            }
        }

        DynamicImage::ImageRgb8(img)
    }

    /// Verifica que ModelDownloader se instancia y reporta modelos faltantes.
    #[test]
    fn test_model_downloader_instanciacion() {
        let resultado = ModelDownloader::new();
        assert!(
            resultado.is_ok(),
            "ModelDownloader debe instanciarse: {:?}",
            resultado.err()
        );

        let downloader = resultado.unwrap();
        let faltantes = downloader.modelos_faltantes();

        if faltantes.is_empty() {
            eprintln!("Todos los modelos disponibles");
        } else {
            eprintln!("Modelos faltantes: {:?}", faltantes);
        }
    }

    /// Test E2E: descarga modelos, inicializa engine, procesa documento.
    ///
    /// ADVERTENCIA: descarga ~100MB de modelos en la primera ejecucion.
    #[test]
    #[ignore]
    fn test_pipeline_completo_con_imagen_sintetica() {
        use ocrfast::domain::ProcessingProfile;
        use ocrfast::infrastructure::document_parsers::image_parser::ImageDocumentParser;
        use ocrfast::infrastructure::ocr_engines::onnx::OnnxOcrEngine;
        use ocrfast::interfaces::ports::{DocumentParserPort, OcrEnginePort};

        let dir_temp = std::env::temp_dir().join("ocrfast_e2e_test");
        std::fs::create_dir_all(&dir_temp).unwrap();
        let ruta_imagen = dir_temp.join("test_e2e.png");
        let imagen = generar_imagen_con_texto_simulado();
        imagen.save(&ruta_imagen).unwrap();

        let parser = ImageDocumentParser::new();
        let mut documento = parser
            .parse(&ruta_imagen)
            .expect("Parse de imagen de prueba debe ser exitoso");

        assert_eq!(documento.pages.len(), 1);
        assert!(documento.pages[0].image_data.is_some());

        let engine =
            OnnxOcrEngine::new().expect("OnnxOcrEngine debe inicializarse (modelos necesarios)");

        let resultado = engine.process(&mut documento, &ProcessingProfile::Balanced);
        assert!(
            resultado.is_ok(),
            "Pipeline completo debe ejecutarse sin error: {:?}",
            resultado.err()
        );

        let total_bloques: usize = documento.pages.iter().map(|p| p.blocks.len()).sum();
        eprintln!("Total bloques detectados: {}", total_bloques);

        assert!(
            total_bloques > 0,
            "El pipeline debe detectar al menos 1 bloque en la imagen de prueba"
        );

        for pagina in &documento.pages {
            for bloque in &pagina.blocks {
                eprintln!(
                    "Bloque: {:?} | Contenido: '{}' | Confianza: {:.2}",
                    bloque.block_type, bloque.content, bloque.confidence
                );
            }
        }

        let _ = std::fs::remove_dir_all(&dir_temp);
    }

    /// Test de layout aislado (DocLayout-YOLO).
    #[test]
    #[ignore]
    fn test_layout_con_imagen_sintetica() {
        use ocrfast::domain::{Dimensions, Page};
        use ocrfast::infrastructure::ocr_engines::onnx::OnnxOcrEngine;
        use ocrfast::interfaces::ports::LayoutEnginePort;

        let imagen = generar_imagen_con_texto_simulado();
        let mut datos_imagen = Vec::new();
        imagen
            .write_to(
                &mut std::io::Cursor::new(&mut datos_imagen),
                image::ImageFormat::Png,
            )
            .unwrap();

        let pagina = Page {
            number: 1,
            dimensions: Dimensions {
                width: 800,
                height: 600,
            },
            blocks: vec![],
            image_data: Some(datos_imagen),
        };

        let engine = OnnxOcrEngine::new().expect("OnnxOcrEngine debe inicializarse");

        let bloques = engine.analyze(&pagina);
        assert!(
            bloques.is_ok(),
            "Layout analysis debe ejecutarse: {:?}",
            bloques.err()
        );

        let bloques = bloques.unwrap();
        eprintln!("Layout detecto {} bloques", bloques.len());

        for bloque in &bloques {
            eprintln!(
                "  [{:?}] x={} y={} w={} h={} conf={:.2}",
                bloque.block_type,
                bloque.bounding_box.x,
                bloque.bounding_box.y,
                bloque.bounding_box.width,
                bloque.bounding_box.height,
                bloque.confidence
            );
        }
    }

    /// Test del downloader: descarga y verifica modelos.
    #[test]
    #[ignore]
    fn test_descarga_modelos_completa() {
        let downloader = ModelDownloader::new().expect("ModelDownloader debe instanciarse");

        let resultado = downloader.asegurar_todos_los_modelos(None, None);
        assert!(
            resultado.is_ok(),
            "Descarga de modelos debe completarse: {:?}",
            resultado.err()
        );

        let ruta = resultado.unwrap();
        assert!(ruta.exists(), "Directorio de modelos debe existir");

        assert!(
            downloader.todos_los_modelos_disponibles(),
            "Todos los modelos deben estar disponibles tras descarga"
        );

        let faltantes = downloader.modelos_faltantes();
        assert!(
            faltantes.is_empty(),
            "No debe haber modelos faltantes: {:?}",
            faltantes
        );
    }
}
