use super::common::{asegurar_directorio_padre, construir_blueprint};
use crate::domain::errors::ExportError;
use crate::domain::{ElementRole, Job};
use crate::interfaces::ports::ExporterPort;
use std::fs;
use std::path::Path;

/// Exportador de documentos OCR a texto plano legible por humanos.
pub struct TxtExporter;

impl TxtExporter {
    /// Construye un exportador TXT sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TxtExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for TxtExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;

        let mut contenido = String::new();
        contenido.push_str(&format!("Documento: {}\n", job.document.id));
        contenido.push_str(&format!(
            "Archivo fuente: {}\n",
            job.document.source_path.display()
        ));
        contenido.push_str(&format!("Perfil: {:?}\n", job.profile));
        contenido.push_str(&format!("Estado: {:?}\n\n", job.status));

        let blueprint = construir_blueprint(&job.document)?;

        for pagina in &blueprint.pages {
            contenido.push_str(&format!("===== PAGINA {} =====\n\n", pagina.number));

            for elemento in &pagina.elements {
                match elemento.role {
                    ElementRole::Title => {
                        contenido.push_str(&format!("{}\n\n", elemento.text.to_uppercase()));
                    }
                    ElementRole::Paragraph | ElementRole::ListItem => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                    ElementRole::Table => {
                        if let Some(ref tabla) = elemento.table {
                            let tabla_txt = tabla.to_plain_text();
                            if !tabla_txt.is_empty() {
                                contenido.push_str(&tabla_txt);
                                contenido.push('\n');
                            }
                        } else if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        } else {
                            contenido.push_str("[Tabla sin contenido]\n\n");
                        }
                    }
                    ElementRole::Formula => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str("Formula: ");
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                    ElementRole::Figure | ElementRole::Signature | ElementRole::Stamp => {
                        contenido.push_str("[Activo visual preservado en exportadores ricos]\n\n");
                    }
                    ElementRole::Separator | ElementRole::Unknown => {
                        if !elemento.text.trim().is_empty() {
                            contenido.push_str(&elemento.text);
                            contenido.push_str("\n\n");
                        }
                    }
                }
            }
        }

        fs::write(output_path, contenido)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "TXT"
    }
}
