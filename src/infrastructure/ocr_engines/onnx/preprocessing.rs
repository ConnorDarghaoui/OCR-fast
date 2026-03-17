//! Preprocesamiento de imagenes para modelos ONNX.
//!
//! Cada modelo espera un tensor con forma y normalizacion especifica.
//! Los loops iteran en orden CHW (canal→fila→columna) para que las
//! escrituras al tensor sean secuenciales en memoria, evitando cache misses.

use image::{DynamicImage, GenericImageView};
use ndarray::Array4;

// Normalizacion ImageNet (orientation, deteccion, tablas)
const MEAN_IMAGENET: [f32; 3] = [0.485, 0.456, 0.406];
const STD_IMAGENET: [f32; 3]  = [0.229, 0.224, 0.225];

/// Escribe los bytes RGB de una imagen redimensionada en un tensor CHW.
///
/// Itera canal→fila→columna para que las escrituras al tensor sean
/// secuenciales. Lee del buffer raw de la imagen con acceso indexado
/// directo, sin el overhead de `get_pixel`.
#[inline]
fn rgb_a_tensor_chw(
    bytes: &[u8],
    tensor: &mut Array4<f32>,
    w: usize,
    h: usize,
    mean: &[f32; 3],
    std: &[f32; 3],
) {
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                tensor[[0, c, y, x]] =
                    (bytes[(y * w + x) * 3 + c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
    }
}

/// Para el modelo de orientacion PP-LCNet.
/// Entrada esperada: `[1, 3, 224, 224]`, normalizado con mean/std ImageNet.
pub fn preparar_para_orientacion(imagen: &DynamicImage) -> Array4<f32> {
    const H: usize = 224;
    const W: usize = 224;

    let img = imagen.resize_exact(W as u32, H as u32, image::imageops::FilterType::Triangle);
    let bytes = img.to_rgb8();

    let mut tensor = Array4::zeros((1, 3, H, W));
    rgb_a_tensor_chw(bytes.as_raw(), &mut tensor, W, H, &MEAN_IMAGENET, &STD_IMAGENET);
    tensor
}

/// Para el modelo de layout DocLayout-YOLO.
/// Entrada esperada: `[1, 3, 1024, 1024]`, normalizado a [0, 1].
/// Preserva aspect ratio con padding gris (114/255, convencion YOLO).
///
/// Retorna `(tensor, escala_inv_x, escala_inv_y)` para mapear coordenadas de vuelta.
pub fn preparar_para_layout(imagen: &DynamicImage) -> (Array4<f32>, f32, f32) {
    const DIM: usize = 1024;
    let (w_orig, h_orig) = imagen.dimensions();

    let escala = (DIM as f32 / w_orig as f32).min(DIM as f32 / h_orig as f32);
    let w_nuevo = (w_orig as f32 * escala) as usize;
    let h_nuevo = (h_orig as f32 * escala) as usize;

    let img = imagen.resize_exact(w_nuevo as u32, h_nuevo as u32, image::imageops::FilterType::Triangle);
    let bytes = img.to_rgb8();

    // Padding YOLO: gris 114/255
    let valor_padding = 114.0f32 / 255.0;
    let mut tensor = Array4::from_elem((1, 3, DIM, DIM), valor_padding);

    for c in 0..3 {
        for y in 0..h_nuevo {
            for x in 0..w_nuevo {
                tensor[[0, c, y, x]] = bytes.as_raw()[(y * w_nuevo + x) * 3 + c] as f32 / 255.0;
            }
        }
    }

    let escala_inv_x = w_orig as f32 / w_nuevo as f32;
    let escala_inv_y = h_orig as f32 / h_nuevo as f32;
    (tensor, escala_inv_x, escala_inv_y)
}

/// Para el detector de texto PaddleOCR v5.
/// Entrada esperada: `[1, 3, H, W]` donde H y W son multiplos de 32.
/// Normalizado con mean/std ImageNet.
pub fn preparar_para_deteccion_texto(imagen: &DynamicImage) -> Array4<f32> {
    let (w, h) = imagen.dimensions();
    let w_nuevo = ((w as f32 / 32.0).ceil() as usize) * 32;
    let h_nuevo = ((h as f32 / 32.0).ceil() as usize) * 32;

    let img = imagen.resize_exact(w_nuevo as u32, h_nuevo as u32, image::imageops::FilterType::Triangle);
    let bytes = img.to_rgb8();

    let mut tensor = Array4::zeros((1, 3, h_nuevo, w_nuevo));
    rgb_a_tensor_chw(bytes.as_raw(), &mut tensor, w_nuevo, h_nuevo, &MEAN_IMAGENET, &STD_IMAGENET);
    tensor
}

/// Para el reconocedor de texto PaddleOCR.
/// Entrada esperada: `[1, 3, 48, W]` donde W es proporcional al ancho original.
/// Normalizado con `(x/255 - 0.5) / 0.5`.
pub fn preparar_para_reconocimiento(recorte: &DynamicImage) -> Array4<f32> {
    const H: usize = 48;
    let (w_orig, h_orig) = recorte.dimensions();
    let w_nuevo = ((w_orig as f32 * H as f32 / h_orig as f32) as usize).max(1);

    let img = recorte.resize_exact(w_nuevo as u32, H as u32, image::imageops::FilterType::Triangle);
    let bytes = img.to_rgb8();

    let mean = [0.5f32; 3];
    let std  = [0.5f32; 3];

    let mut tensor = Array4::zeros((1, 3, H, w_nuevo));
    rgb_a_tensor_chw(bytes.as_raw(), &mut tensor, w_nuevo, H, &mean, &std);
    tensor
}

/// Para el Table Transformer (DETR).
/// Entrada esperada: `[1, 3, 800, 800]`, normalizado con mean/std ImageNet (COCO).
pub fn preparar_para_tabla(recorte: &DynamicImage) -> Array4<f32> {
    const H: usize = 800;
    const W: usize = 800;

    let img = recorte.resize_exact(W as u32, H as u32, image::imageops::FilterType::Triangle);
    let bytes = img.to_rgb8();

    let mut tensor = Array4::zeros((1, 3, H, W));
    rgb_a_tensor_chw(bytes.as_raw(), &mut tensor, W, H, &MEAN_IMAGENET, &STD_IMAGENET);
    tensor
}
