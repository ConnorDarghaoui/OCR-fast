use super::common::asegurar_directorio_padre;
use crate::domain::errors::ExportError;
use crate::domain::Job;
use crate::interfaces::ports::ExporterPort;
use std::fs;
use std::path::Path;

/// Exportador a JSON estructurado para integración y depuración.
pub struct JsonExporter;

impl JsonExporter {
    /// Crea un nuevo exportador JSON.
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExporterPort for JsonExporter {
    fn export(&self, job: &Job, output_path: &Path) -> Result<(), ExportError> {
        asegurar_directorio_padre(output_path)?;
        let json_content = serde_json::to_string_pretty(job)
            .map_err(|e| ExportError::SerializationError(e.to_string()))?;
        fs::write(output_path, json_content)?;
        Ok(())
    }

    fn format_name(&self) -> &str {
        "JSON"
    }
}
