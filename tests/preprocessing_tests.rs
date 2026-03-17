//! Tests unitarios de funciones de preprocesamiento ONNX.
//!
//! Validan formas de tensores, rangos de normalizacion y
//! propiedades geometricas (aspect ratio, padding, multiplos de 32).
//! No requieren modelos ONNX descargados.

#[cfg(test)]
mod tests {
    use image::{DynamicImage, RgbImage};
    use ocrfast::infrastructure::ocr_engines::onnx::preprocessing;

    /// Genera una imagen sintetica RGB de dimensiones arbitrarias.
    fn generar_imagen(ancho: u32, alto: u32) -> DynamicImage {
        let mut img = RgbImage::new(ancho, alto);
        // Patron tipo checkerboard para variacion de pixeles
        for y in 0..alto {
            for x in 0..ancho {
                let valor = if (x + y) % 2 == 0 { 200 } else { 50 };
                img.put_pixel(x, y, image::Rgb([valor, valor, valor]));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    // --- Orientacion (PP-LCNet) ---

    #[test]
    fn test_orientacion_forma_correcta() {
        let imagen = generar_imagen(640, 480);
        let tensor = preprocessing::preparar_para_orientacion(&imagen);
        assert_eq!(tensor.shape(), &[1, 3, 224, 224]);
    }

    #[test]
    fn test_orientacion_valores_normalizados() {
        let imagen = generar_imagen(100, 100);
        let tensor = preprocessing::preparar_para_orientacion(&imagen);

        for valor in tensor.iter() {
            assert!(valor.is_finite(), "Valor no finito en tensor de orientacion");
            // ImageNet norm: rango tipico aprox [-2.5, 2.5]
            assert!(
                *valor >= -3.0 && *valor <= 3.0,
                "Valor fuera de rango esperado: {}",
                valor
            );
        }
    }

    // --- Layout (DocLayout-YOLO) ---

    #[test]
    fn test_layout_forma_correcta() {
        let imagen = generar_imagen(800, 600);
        let (tensor, escala_inv_x, escala_inv_y) = preprocessing::preparar_para_layout(&imagen);
        assert_eq!(tensor.shape(), &[1, 3, 1024, 1024]);
        assert!(escala_inv_x > 0.0, "Escala inversa X debe ser positiva");
        assert!(escala_inv_y > 0.0, "Escala inversa Y debe ser positiva");
    }

    #[test]
    fn test_layout_escalas_inversas_coherentes() {
        let imagen = generar_imagen(1920, 1080);
        let (_tensor, escala_inv_x, escala_inv_y) = preprocessing::preparar_para_layout(&imagen);

        // Las escalas inversas deben permitir reconstruir coordenadas originales
        // Para una imagen landscape, escala_x >= escala_y
        assert!(
            escala_inv_x > 0.5 && escala_inv_x < 10.0,
            "Escala inversa X fuera de rango razonable: {}",
            escala_inv_x
        );
        assert!(
            escala_inv_y > 0.5 && escala_inv_y < 10.0,
            "Escala inversa Y fuera de rango razonable: {}",
            escala_inv_y
        );
    }

    #[test]
    fn test_layout_valores_en_rango_0_1() {
        let imagen = generar_imagen(200, 200);
        let (tensor, _, _) = preprocessing::preparar_para_layout(&imagen);

        for valor in tensor.iter() {
            assert!(valor.is_finite(), "Valor no finito en tensor de layout");
            // Layout normaliza a [0, 1] con padding gris ~0.447
            assert!(
                *valor >= 0.0 && *valor <= 1.0,
                "Valor fuera de rango [0,1]: {}",
                valor
            );
        }
    }

    // --- Deteccion de Texto (PaddleOCR) ---

    #[test]
    fn test_deteccion_texto_dimensiones_multiplo_32() {
        let imagen = generar_imagen(300, 250);
        let tensor = preprocessing::preparar_para_deteccion_texto(&imagen);
        let forma = tensor.shape();

        assert_eq!(forma[0], 1, "Batch size debe ser 1");
        assert_eq!(forma[1], 3, "Canales debe ser 3");
        assert_eq!(
            forma[2] % 32,
            0,
            "Altura {} no es multiplo de 32",
            forma[2]
        );
        assert_eq!(
            forma[3] % 32,
            0,
            "Ancho {} no es multiplo de 32",
            forma[3]
        );
    }

    #[test]
    fn test_deteccion_texto_imagen_ya_multiplo_32() {
        let imagen = generar_imagen(640, 480);
        let tensor = preprocessing::preparar_para_deteccion_texto(&imagen);
        let forma = tensor.shape();

        assert_eq!(forma[2], 480, "Altura ya multiplo de 32 no deberia cambiar");
        assert_eq!(forma[3], 640, "Ancho ya multiplo de 32 no deberia cambiar");
    }

    // --- Reconocimiento de Texto (PaddleOCR) ---

    #[test]
    fn test_reconocimiento_altura_fija_48() {
        let recorte = generar_imagen(200, 30);
        let tensor = preprocessing::preparar_para_reconocimiento(&recorte);
        let forma = tensor.shape();

        assert_eq!(forma[0], 1, "Batch size debe ser 1");
        assert_eq!(forma[1], 3, "Canales debe ser 3");
        assert_eq!(forma[2], 48, "Altura debe ser fija en 48");
        // Ancho proporcional: 200 * (48/30) = 320
        assert!(forma[3] > 0, "Ancho debe ser > 0");
    }

    #[test]
    fn test_reconocimiento_ancho_proporcional() {
        let recorte_ancho = generar_imagen(400, 40);
        let recorte_estrecho = generar_imagen(100, 40);

        let tensor_ancho = preprocessing::preparar_para_reconocimiento(&recorte_ancho);
        let tensor_estrecho = preprocessing::preparar_para_reconocimiento(&recorte_estrecho);

        assert!(
            tensor_ancho.shape()[3] > tensor_estrecho.shape()[3],
            "Recorte mas ancho debe producir tensor mas ancho"
        );
    }

    // --- Tabla (Table Transformer DETR) ---

    #[test]
    fn test_tabla_forma_correcta() {
        let imagen = generar_imagen(500, 300);
        let tensor = preprocessing::preparar_para_tabla(&imagen);
        assert_eq!(tensor.shape(), &[1, 3, 800, 800]);
    }

    #[test]
    fn test_tabla_valores_normalizados() {
        let imagen = generar_imagen(100, 100);
        let tensor = preprocessing::preparar_para_tabla(&imagen);

        for valor in tensor.iter() {
            assert!(valor.is_finite(), "Valor no finito en tensor de tabla");
            // COCO normalizacion, rango aprox [-2.5, 2.5]
            assert!(
                *valor >= -3.0 && *valor <= 3.0,
                "Valor fuera de rango: {}",
                valor
            );
        }
    }
}
