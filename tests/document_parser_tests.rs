#[cfg(test)]
mod tests {
    use ocrfast::infrastructure::document_parsers::image_parser::ImageDocumentParser;
    use ocrfast::interfaces::ports::DocumentParserPort;
    use std::path::PathBuf;

    #[test]
    fn test_parser_se_instancia_correctamente() {
        let _parser = ImageDocumentParser::new();
        assert!(true, "ImageDocumentParser::new() no debe fallar");
    }

    #[test]
    fn test_parser_archivo_inexistente_retorna_error() {
        let parser = ImageDocumentParser::new();
        let ruta = PathBuf::from("/tmp/archivo_que_no_existe_ocrfast_test.png");

        let resultado = parser.parse(&ruta);
        assert!(
            resultado.is_err(),
            "Parse de archivo inexistente debe retornar error"
        );
    }

    #[test]
    fn test_parser_imagen_sintetica_png() {
        use image::{DynamicImage, RgbImage};

        let dir_temp = std::env::temp_dir().join("ocrfast_test_parse");
        std::fs::create_dir_all(&dir_temp).unwrap();
        let ruta_imagen = dir_temp.join("test_sintetico.png");

        let mut img = RgbImage::new(200, 100);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([255, 255, 255]);
        }
        let imagen_dyn = DynamicImage::ImageRgb8(img);
        imagen_dyn.save(&ruta_imagen).unwrap();

        let parser = ImageDocumentParser::new();
        let resultado = parser.parse(&ruta_imagen);

        assert!(
            resultado.is_ok(),
            "Parse de PNG valido debe ser exitoso: {:?}",
            resultado.err()
        );

        let documento = resultado.unwrap();
        assert_eq!(
            documento.pages.len(),
            1,
            "Imagen simple debe producir 1 pagina"
        );
        assert_eq!(documento.pages[0].dimensions.width, 200);
        assert_eq!(documento.pages[0].dimensions.height, 100);
        assert!(
            documento.pages[0].image_data.is_some(),
            "La pagina debe contener image_data"
        );

        let _ = std::fs::remove_dir_all(&dir_temp);
    }

    #[test]
    fn test_parser_formato_no_soportado() {
        let dir_temp = std::env::temp_dir().join("ocrfast_test_format");
        std::fs::create_dir_all(&dir_temp).unwrap();
        let ruta = dir_temp.join("archivo.xyz");
        std::fs::write(&ruta, b"contenido invalido").unwrap();

        let parser = ImageDocumentParser::new();
        let resultado = parser.parse(&ruta);

        assert!(
            resultado.is_err(),
            "Formato no soportado debe retornar error"
        );

        let _ = std::fs::remove_dir_all(&dir_temp);
    }

    /// Verifica que un TIFF de una sola pagina se parsea correctamente
    /// (sin regresar a comportamiento de pagina-unica).
    #[test]
    fn test_parser_tiff_pagina_unica() {
        use image::{DynamicImage, RgbImage};

        let dir_temp = std::env::temp_dir().join("ocrfast_test_tiff_single");
        std::fs::create_dir_all(&dir_temp).unwrap();
        let ruta = dir_temp.join("single.tiff");

        let img = DynamicImage::ImageRgb8(RgbImage::new(100, 80));
        img.save(&ruta).unwrap();

        let parser = ImageDocumentParser::new();
        let resultado = parser.parse(&ruta);

        assert!(
            resultado.is_ok(),
            "TIFF valido debe parsearse: {:?}",
            resultado.err()
        );
        let doc = resultado.unwrap();
        assert_eq!(
            doc.pages.len(),
            1,
            "TIFF de 1 pagina debe producir 1 pagina"
        );
        assert_eq!(doc.pages[0].dimensions.width, 100);
        assert_eq!(doc.pages[0].dimensions.height, 80);
        assert!(doc.pages[0].image_data.is_some());

        let _ = std::fs::remove_dir_all(&dir_temp);
    }

    /// Verifica soporte multi-pagina en TIFF.
    ///
    /// Genera un TIFF con 2 paginas de distintas dimensiones
    /// y verifica que el parser produce 2 Pages en el Document.
    #[test]
    fn test_parser_tiff_multipagina() {
        use std::io::BufWriter;
        use tiff::encoder::{colortype, TiffEncoder};

        let dir_temp = std::env::temp_dir().join("ocrfast_test_tiff_multi");
        std::fs::create_dir_all(&dir_temp).unwrap();
        let ruta = dir_temp.join("multi.tiff");

        {
            let archivo = std::fs::File::create(&ruta).unwrap();
            let mut encoder = TiffEncoder::new(BufWriter::new(archivo)).unwrap();

            let pixeles_p1: Vec<u8> = vec![255u8; 120 * 80 * 3];
            encoder
                .write_image::<colortype::RGB8>(120, 80, &pixeles_p1)
                .unwrap();

            let pixeles_p2: Vec<u8> = vec![128u8; 200 * 150 * 3];
            encoder
                .write_image::<colortype::RGB8>(200, 150, &pixeles_p2)
                .unwrap();
        }

        let parser = ImageDocumentParser::new();
        let resultado = parser.parse(&ruta);

        assert!(
            resultado.is_ok(),
            "TIFF multi-pagina debe parsearse: {:?}",
            resultado.err()
        );

        let doc = resultado.unwrap();
        assert_eq!(
            doc.pages.len(),
            2,
            "TIFF de 2 paginas debe producir 2 Pages en el Document"
        );

        assert_eq!(doc.pages[0].number, 1);
        assert_eq!(doc.pages[0].dimensions.width, 120);
        assert_eq!(doc.pages[0].dimensions.height, 80);
        assert!(doc.pages[0].image_data.is_some());

        assert_eq!(doc.pages[1].number, 2);
        assert_eq!(doc.pages[1].dimensions.width, 200);
        assert_eq!(doc.pages[1].dimensions.height, 150);
        assert!(doc.pages[1].image_data.is_some());

        let _ = std::fs::remove_dir_all(&dir_temp);
    }
}
