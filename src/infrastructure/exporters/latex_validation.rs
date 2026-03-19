use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Variable de entorno para localizar un binario `tectonic` no estándar.
pub const OCRFAST_TECTONIC_BIN_ENV: &str = "OCRFAST_TECTONIC_BIN";
/// Variable de entorno para activar tests que compilan LaTeX de verdad.
pub const OCRFAST_RUN_TECTONIC_TESTS_ENV: &str = "OCRFAST_RUN_TECTONIC_TESTS";

/// Artefactos generados por una compilación LaTeX exitosa.
pub struct LatexCompilationArtifacts {
    /// PDF materializado por el compilador.
    pub pdf_path: PathBuf,
    /// Log emitido por `tectonic`, si el compilador lo produjo.
    pub log_path: Option<PathBuf>,
}

/// Fallos propagables al invocar el compilador LaTeX externo.
#[derive(Debug, thiserror::Error)]
pub enum LatexCompilerError {
    /// No se pudo encontrar ni ejecutar un binario `tectonic` compatible.
    #[error("No se encontró un binario tectonic utilizable")]
    BinaryUnavailable,
    /// Error local de entrada/salida al preparar o leer artefactos.
    #[error("Fallo de E/S en validación LaTeX: {0}")]
    Io(#[from] std::io::Error),
    /// El compilador terminó con error y reportó salida diagnóstica.
    #[error("La compilación LaTeX falló con status {status:?}: {stderr}")]
    CompilationFailed {
        /// Código de salida del proceso hijo.
        status: Option<i32>,
        /// STDERR decodificado como UTF-8 lossy.
        stderr: String,
    },
}

/// Puente opcional hacia compilación real de LaTeX usando `tectonic`.
///
/// El validador permanece fuera del camino principal de exportación: solo se
/// activa mediante la feature `latex_compiler_validation` y, en tests, además
/// requiere una variable de entorno explícita para evitar dependencias duras en
/// entornos donde `tectonic` no exista o no deba ejecutarse.
pub struct LatexCompilerValidator {
    binary_path: OsString,
}

impl LatexCompilerValidator {
    /// Intenta descubrir un binario `tectonic` utilizable desde el entorno.
    pub fn discover() -> Option<Self> {
        let candidato = std::env::var_os(OCRFAST_TECTONIC_BIN_ENV)
            .filter(|valor| !valor.is_empty())
            .unwrap_or_else(|| OsString::from("tectonic"));

        let probe = Command::new(&candidato).arg("--version").output().ok()?;
        if probe.status.success() {
            Some(Self {
                binary_path: candidato,
            })
        } else {
            None
        }
    }

    /// Indica si los tests integrados de compilación real deben ejecutarse.
    pub fn tests_enabled() -> bool {
        matches!(
            std::env::var(OCRFAST_RUN_TECTONIC_TESTS_ENV)
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        )
    }

    /// Compila una fuente `.tex` ya materializada y retorna los artefactos.
    pub fn compile_tex_file(
        &self,
        tex_path: &Path,
    ) -> Result<LatexCompilationArtifacts, LatexCompilerError> {
        let workdir = tex_path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "La ruta .tex debe tener directorio padre",
            )
        })?;
        let file_name = tex_path.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "La ruta .tex debe tener nombre de archivo",
            )
        })?;
        let stem = tex_path
            .file_stem()
            .and_then(|valor| valor.to_str())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "La ruta .tex debe tener stem UTF-8",
                )
            })?;

        let output = Command::new(&self.binary_path)
            .current_dir(workdir)
            .arg("--keep-logs")
            .arg("--keep-intermediates")
            .arg("--outdir")
            .arg(workdir)
            .arg(file_name)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    LatexCompilerError::BinaryUnavailable
                } else {
                    LatexCompilerError::Io(error)
                }
            })?;

        if !output.status.success() {
            return Err(LatexCompilerError::CompilationFailed {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        let pdf_path = workdir.join(format!("{stem}.pdf"));
        if !pdf_path.exists() {
            return Err(LatexCompilerError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "tectonic no produjo el PDF esperado",
            )));
        }

        let log_path = {
            let candidato = workdir.join(format!("{stem}.log"));
            candidato.exists().then_some(candidato)
        };

        Ok(LatexCompilationArtifacts { pdf_path, log_path })
    }
}
