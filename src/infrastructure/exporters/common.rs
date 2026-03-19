use crate::domain::errors::ExportError;
use crate::domain::{Document, DocumentBlueprint, Job, Page, Rectangle};
use crate::infrastructure::page_composer::PageComposer;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// DPI asumido para convertir geometría raster a tamaños tipográficos.
pub(super) const DPI_REFERENCIA: f64 = 150.0;
/// Factor de conversión: 1 punto PDF = 1/72 pulgadas.
pub(super) const PUNTOS_POR_PULGADA: f64 = 72.0;

pub(super) fn construir_blueprint(documento: &Document) -> Result<DocumentBlueprint, ExportError> {
    PageComposer::new().compose(documento).map_err(|e| {
        ExportError::SerializationError(format!("No se pudo construir blueprint: {e}"))
    })
}

pub(super) fn asegurar_directorio_padre(ruta: &Path) -> Result<(), ExportError> {
    if let Some(parent) = ruta.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(super) fn directorio_assets(ruta_salida: &Path) -> PathBuf {
    let stem = ruta_salida
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("documento");
    ruta_salida.with_file_name(format!("{stem}_assets"))
}

pub(super) fn px_a_pt(px: u32) -> f64 {
    (px as f64) * (PUNTOS_POR_PULGADA / DPI_REFERENCIA)
}

pub(super) fn obtener_pagina<'a>(
    job: &'a Job,
    numero_pagina: u32,
) -> Result<&'a Page, ExportError> {
    job.document
        .pages
        .iter()
        .find(|pagina| pagina.number == numero_pagina)
        .ok_or_else(|| {
            ExportError::SerializationError(format!(
                "No existe la pagina {numero_pagina} en el documento"
            ))
        })
}

pub(super) fn recortar_imagen_desde_referencia(
    job: &Job,
    numero_pagina: u32,
    bounding_box: &Rectangle,
) -> Result<Vec<u8>, ExportError> {
    let pagina = obtener_pagina(job, numero_pagina)?;
    let datos = pagina.image_data.as_ref().ok_or_else(|| {
        ExportError::SerializationError(format!(
            "La pagina {numero_pagina} no conserva raster en memoria"
        ))
    })?;

    let imagen = image::load_from_memory(datos).map_err(|e| {
        ExportError::SerializationError(format!("No se pudo decodificar raster: {e}"))
    })?;

    let ancho = imagen.width();
    let alto = imagen.height();
    let x = bounding_box.x.min(ancho);
    let y = bounding_box.y.min(alto);
    let w = bounding_box.width.min(ancho.saturating_sub(x)).max(1);
    let h = bounding_box.height.min(alto.saturating_sub(y)).max(1);

    let recorte = imagen.crop_imm(x, y, w, h);
    let mut cursor = Cursor::new(Vec::new());
    recorte
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| {
            ExportError::SerializationError(format!("No se pudo codificar recorte PNG: {e}"))
        })?;

    Ok(cursor.into_inner())
}
