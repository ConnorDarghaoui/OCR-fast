mod common;
mod json;
mod latex;
mod latex_ast;
#[cfg(feature = "latex_compiler_validation")]
mod latex_validation;
mod pdf;
mod txt;

use crate::domain::{Job, OutputFormat};
use crate::interfaces::ports::{ExporterPort, JobExporterPort};
use std::path::Path;
use std::sync::Arc;

pub use crate::interfaces::ports::ExporterPort as Exporter;
pub use json::JsonExporter;
pub use latex::LatexExporter;
#[cfg(feature = "latex_compiler_validation")]
pub use latex_validation::{
    LatexCompilationArtifacts, LatexCompilerError, LatexCompilerValidator,
    OCRFAST_RUN_TECTONIC_TESTS_ENV, OCRFAST_TECTONIC_BIN_ENV,
};
pub use pdf::PdfReconstructedExporter;
pub use txt::TxtExporter;

/// Registro por defecto que resuelve exportadores concretos según `OutputFormat`.
///
/// La resolución vive en infraestructura para que la aplicación no dependa de
/// constructores concretos ni replique el `match` en múltiples sitios.
pub struct DefaultJobExporter {
    txt: Arc<dyn ExporterPort>,
    latex: Arc<dyn ExporterPort>,
    pdf: Arc<dyn ExporterPort>,
    json: Arc<dyn ExporterPort>,
}

impl DefaultJobExporter {
    /// Construye el registro con los exportadores integrados del producto.
    pub fn new() -> Self {
        Self {
            txt: Arc::new(TxtExporter::new()),
            latex: Arc::new(LatexExporter::new()),
            pdf: Arc::new(PdfReconstructedExporter::new()),
            json: Arc::new(JsonExporter::new()),
        }
    }

    /// Resuelve el exportador concreto para el formato indicado.
    fn exportador_para(&self, formato: OutputFormat) -> &dyn ExporterPort {
        match formato {
            OutputFormat::Txt => self.txt.as_ref(),
            OutputFormat::Latex => self.latex.as_ref(),
            OutputFormat::Pdf => self.pdf.as_ref(),
            OutputFormat::Json => self.json.as_ref(),
        }
    }
}

impl Default for DefaultJobExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JobExporterPort for DefaultJobExporter {
    fn export_job(
        &self,
        job: &Job,
        output_path: &Path,
    ) -> Result<(), crate::domain::errors::ExportError> {
        self.exportador_para(job.formato_salida)
            .export(job, output_path)
    }
}
