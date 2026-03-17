use crate::domain::errors::OcrError;
use crate::domain::Document;
use crate::interfaces::ports::PostprocessorPort;

/// Postprocesador textual configurable para limpieza y normalización OCR.
///
/// Separa heurísticas lingüísticas del engine OCR para que la calidad textual
/// pueda evolucionar sin reentrenar modelos ni mezclar reglas de negocio con
/// inferencia. El diseño basado en flags permite desactivar transformaciones con
/// riesgo en contextos donde fidelidad literal importe más que legibilidad.
pub struct TextPostprocessor {
    /// Aplicar normalizacion Unicode.
    normalize_unicode: bool,
    /// Aplicar correccion de espaciado.
    fix_spacing: bool,
    /// Aplicar correccion por diccionario.
    use_dictionary: bool,
    /// Codigo ISO 639-3 del idioma primario del documento (ej: "spa", "eng").
    /// Determina que tabla de correcciones de palabra se activa.
    language: String,
}

impl TextPostprocessor {
    /// Crea un postprocesador con defaults seguros para uso general.
    pub fn new() -> Self {
        Self {
            normalize_unicode: true,
            fix_spacing: true,
            use_dictionary: false,
            language: "spa".to_string(),
        }
    }

    /// Crea un postprocesador con configuración explícita de transformaciones.
    pub fn with_config(normalize_unicode: bool, fix_spacing: bool, use_dictionary: bool) -> Self {
        Self {
            normalize_unicode,
            fix_spacing,
            use_dictionary,
            language: "spa".to_string(),
        }
    }

    /// Selecciona el idioma base del diccionario de correcciones.
    pub fn with_language(mut self, language: &str) -> Self {
        self.language = language.to_string();
        self
    }

    /// Normaliza texto a Unicode NFC.
    fn normalize(&self, text: &str) -> String {
        use unicode_normalization::UnicodeNormalization;
        text.nfc().collect()
    }

    /// Corrige espaciado multiple y trim.
    fn fix_spaces(&self, text: &str) -> String {
        // Reemplazar espacios multiples por uno solo
        let mut result = String::with_capacity(text.len());
        let mut prev_space = false;

        for c in text.chars() {
            if c.is_whitespace() {
                if !prev_space {
                    result.push(' ');
                    prev_space = true;
                }
            } else {
                result.push(c);
                prev_space = false;
            }
        }

        result.trim().to_string()
    }

    /// Corrige errores OCR comunes usando tabla de confusiones.
    ///
    /// Aplica correcciones en dos fases:
    /// - Fase 1: Correcciones de secuencia Unicode (ligaduras, comillas tipograficas,
    ///   guiones especiales). Son language-agnostic y de riesgo cero.
    /// - Fase 2: Correcciones de palabra completa, seleccionadas segun `self.language`.
    ///   Cada idioma tiene su propio conjunto para evitar falsos positivos cross-idioma
    ///   (ej: "arid" es una palabra valida en espanol, no debe corregirse a "and").
    fn dictionary_correct(&self, text: &str) -> String {
        // Correcciones de secuencia: SOLO sustituciones con riesgo cero de falso positivo.
        // EXCLUIDOS intencionalmente: "rn"→"m", "vv"→"w" (producen falsos positivos
        // en palabras comunes: barn→bam, internal→intemal, arrive→arri\u{0077}e).
        const CORRECCIONES_SECUENCIA: &[(&str, &str)] = &[
            // Ligaduras mal decodificadas (Unicode -> ASCII equivalente)
            ("\u{FB01}", "fi"),  // fi ligature
            ("\u{FB02}", "fl"),  // fl ligature
            ("\u{FB03}", "ffi"), // ffi ligature
            ("\u{FB04}", "ffl"), // ffl ligature
            // Comillas tipograficas -> comillas ASCII (inofensivo, mejora busqueda)
            ("\u{2018}", "'"),  // left single quote -> apostrophe
            ("\u{2019}", "'"),  // right single quote -> apostrophe
            ("\u{201C}", "\""), // left double quote
            ("\u{201D}", "\""), // right double quote
            // Guiones especiales -> guion simple
            ("\u{2014}", "-"), // em dash
            ("\u{2013}", "-"), // en dash
            ("\u{00AD}", ""),  // soft hyphen (caracter invisible, eliminar)
        ];

        // Correcciones de palabra completa para ingles (ISO 639-3: "eng").
        // No aplicar en documentos espanoles: "arid" y "lias" son palabras validas en spa.
        const CORRECCIONES_PALABRA_ENG: &[(&str, &str)] = &[
            ("tbe", "the"),
            ("tlie", "the"),
            ("arid", "and"),
            ("lias", "has"),
            ("liave", "have"),
            ("witli", "with"),
            ("wliich", "which"),
        ];

        // Correcciones de palabra completa para espanol (ISO 639-3: "spa").
        // Errores tipicos de OCR en documentos latinoamericanos/espanoles.
        const CORRECCIONES_PALABRA_SPA: &[(&str, &str)] = &[
            ("quc", "que"),
            ("cstc", "este"),
            ("csta", "esta"),
            ("dcl", "del"),
            ("csto", "esto"),
        ];

        let mut resultado = text.to_string();

        // Fase 1: Correcciones de secuencia (language-agnostic)
        for (patron, reemplazo) in CORRECCIONES_SECUENCIA {
            resultado = resultado.replace(patron, reemplazo);
        }

        // Fase 2: Seleccionar diccionario segun idioma configurado
        let correcciones_palabra: &[(&str, &str)] = match self.language.as_str() {
            "eng" => CORRECCIONES_PALABRA_ENG,
            "spa" => CORRECCIONES_PALABRA_SPA,
            // Idioma no mapeado: omitir correcciones de palabra para evitar falsos positivos
            _ => &[],
        };

        if correcciones_palabra.is_empty() {
            return resultado;
        }

        let palabras: Vec<&str> = resultado.split_whitespace().collect();
        let corregidas: Vec<String> = palabras
            .iter()
            .map(|palabra| {
                let minuscula = palabra.to_lowercase();
                for (error, correccion) in correcciones_palabra {
                    if minuscula == *error {
                        return correccion.to_string();
                    }
                }
                palabra.to_string()
            })
            .collect();

        corregidas.join(" ")
    }
}

impl Default for TextPostprocessor {
    fn default() -> Self {
        Self::new()
    }
}

impl PostprocessorPort for TextPostprocessor {
    fn postprocess(&self, document: &mut Document) -> Result<(), OcrError> {
        log::info!(
            "Postprocesando documento {} ({} paginas)",
            document.id,
            document.pages.len()
        );

        for page in &mut document.pages {
            for block in &mut page.blocks {
                let mut text = block.content.clone();

                if self.normalize_unicode {
                    text = self.normalize(&text);
                }

                if self.fix_spacing {
                    text = self.fix_spaces(&text);
                }

                if self.use_dictionary {
                    text = self.dictionary_correct(&text);
                }

                block.content = text;
            }
        }

        log::info!("Postprocesamiento completado");
        Ok(())
    }
}
