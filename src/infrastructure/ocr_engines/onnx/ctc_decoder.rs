use std::fs;
use std::path::Path;

/// Carga el vocabulario CTC desde el archivo `dict.txt`.
///
/// El índice cero se reserva para el token blank, por lo que el archivo solo
/// contiene símbolos efectivos del vocabulario.
pub fn cargar_diccionario(ruta: &Path) -> Result<Vec<String>, std::io::Error> {
    let contenido = fs::read_to_string(ruta)?;
    let diccionario: Vec<String> = contenido.lines().map(|linea| linea.to_string()).collect();

    log::info!("Diccionario cargado: {} caracteres", diccionario.len());
    Ok(diccionario)
}

/// Decodifica logits CTC usando greedy decoding clásico.
///
/// # Trade-offs
///
/// El algoritmo greedy es muy barato y suficiente para OCR latino generalista,
/// pero renuncia a mejoras potenciales de beam search en casos ambiguos.
pub fn decodificar_ctc(
    predicciones: &[f32],
    tamano_vocabulario: usize,
    diccionario: &[String],
) -> String {
    let mut resultado = String::new();
    let mut indice_anterior: usize = 0; // 0 = blank token

    // Iterar sobre cada timestep
    for chunk in predicciones.chunks(tamano_vocabulario) {
        // Encontrar el indice con mayor probabilidad (argmax)
        let (indice_maximo, _puntuacion) = chunk
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));

        // Reglas CTC:
        // - Ignorar blank tokens (indice 0)
        // - Ignorar repeticiones consecutivas
        if indice_maximo != 0 && indice_maximo != indice_anterior {
            // Los indices del diccionario estan desplazados en 1
            // (indice 0 = blank, indice 1 = primer caracter)
            let indice_dict = indice_maximo - 1;

            if indice_dict < diccionario.len() {
                resultado.push_str(&diccionario[indice_dict]);
            }
        }

        indice_anterior = indice_maximo;
    }

    resultado
}

/// Decodifica logits CTC y calcula una confianza promedio por carácter emitido.
///
/// La confianza se computa sobre caracteres efectivamente retenidos tras las
/// reglas CTC, lo que la vuelve más útil para ranking de resultados que una media
/// cruda sobre todos los timesteps.
pub fn decodificar_ctc_con_confianza(
    predicciones: &[f32],
    tamano_vocabulario: usize,
    diccionario: &[String],
) -> (String, f64) {
    let mut resultado = String::new();
    let mut indice_anterior: usize = 0;
    let mut suma_confianza = 0.0f64;
    let mut conteo_caracteres = 0u32;

    for chunk in predicciones.chunks(tamano_vocabulario) {
        // Softmax para obtener probabilidades normalizadas
        let max_val = chunk.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = chunk.iter().map(|&x| (x - max_val).exp()).sum();

        let (indice_maximo, puntuacion_raw) = chunk
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));

        let probabilidad = (puntuacion_raw - max_val).exp() / exp_sum;

        if indice_maximo != 0 && indice_maximo != indice_anterior {
            let indice_dict = indice_maximo - 1;

            if indice_dict < diccionario.len() {
                resultado.push_str(&diccionario[indice_dict]);
                suma_confianza += probabilidad as f64;
                conteo_caracteres += 1;
            }
        }

        indice_anterior = indice_maximo;
    }

    let confianza_promedio = if conteo_caracteres > 0 {
        suma_confianza / conteo_caracteres as f64
    } else {
        0.0
    };

    (resultado, confianza_promedio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decodificar_ctc_basico() {
        let diccionario = vec![
            "h".to_string(),
            "o".to_string(),
            "l".to_string(),
            "a".to_string(),
        ];

        // Simular salida del modelo: 5 clases (blank + 4 chars), 6 timesteps
        // Secuencia esperada: blank, h, o, l, a, blank -> "hola"
        let predicciones = vec![
            // timestep 0: blank (indice 0)
            1.0, 0.0, 0.0, 0.0, 0.0, // timestep 1: h (indice 1)
            0.0, 1.0, 0.0, 0.0, 0.0, // timestep 2: o (indice 2)
            0.0, 0.0, 1.0, 0.0, 0.0, // timestep 3: l (indice 3)
            0.0, 0.0, 0.0, 1.0, 0.0, // timestep 4: a (indice 4)
            0.0, 0.0, 0.0, 0.0, 1.0, // timestep 5: blank
            1.0, 0.0, 0.0, 0.0, 0.0,
        ];

        let resultado = decodificar_ctc(&predicciones, 5, &diccionario);
        assert_eq!(resultado, "hola");
    }

    #[test]
    fn test_decodificar_ctc_con_repeticiones() {
        let diccionario = vec![
            "h".to_string(),
            "o".to_string(),
            "l".to_string(),
            "a".to_string(),
        ];

        // Con repeticiones: h, h, o, o, l, a -> "hola" (elimina repetidos)
        let predicciones = vec![
            0.0, 1.0, 0.0, 0.0, 0.0, // h
            0.0, 1.0, 0.0, 0.0, 0.0, // h (repetido, ignorado)
            0.0, 0.0, 1.0, 0.0, 0.0, // o
            0.0, 0.0, 1.0, 0.0, 0.0, // o (repetido, ignorado)
            0.0, 0.0, 0.0, 1.0, 0.0, // l
            0.0, 0.0, 0.0, 0.0, 1.0, // a
        ];

        let resultado = decodificar_ctc(&predicciones, 5, &diccionario);
        assert_eq!(resultado, "hola");
    }
}
