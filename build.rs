use flate2::read::GzDecoder;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Version fijada de pdfium-binaries (pinned, no latest).
const PDFIUM_VERSION: &str = "chromium/7678";

/// Determina el nombre del asset segun la plataforma de compilacion.
fn obtener_nombre_asset() -> &'static str {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    match (os, arch) {
        ("linux", "x86_64") => "pdfium-linux-x64.tgz",
        ("linux", "aarch64") => "pdfium-linux-arm64.tgz",
        ("macos", "x86_64") => "pdfium-mac-x64.tgz",
        ("macos", "aarch64") => "pdfium-mac-arm64.tgz",
        ("windows", "x86_64") => "pdfium-win-x64.tgz",
        ("windows", "x86") => "pdfium-win-x86.tgz",
        _ => panic!(
            "Plataforma no soportada: os={}, arch={}. \
             Descargue libpdfium manualmente de https://github.com/bblanchon/pdfium-binaries/releases",
            os, arch
        ),
    }
}

/// Nombre de la libreria segun el sistema operativo.
fn obtener_nombre_libreria() -> &'static str {
    match env::consts::OS {
        "linux" => "libpdfium.so",
        "macos" => "libpdfium.dylib",
        "windows" => "pdfium.dll",
        _ => "libpdfium.so",
    }
}

/// Descarga el archivo .tgz a un path temporal.
fn descargar_pdfium(url: &str, destino: &Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("cargo:warning=Descargando libpdfium de {}", url);

    let respuesta = reqwest::blocking::get(url)?;
    if !respuesta.status().is_success() {
        return Err(format!(
            "Error descargando pdfium: HTTP {}. URL: {}",
            respuesta.status(),
            url
        )
        .into());
    }

    let bytes = respuesta.bytes()?;
    fs::write(destino, &bytes)?;

    eprintln!(
        "cargo:warning=Descargado {} ({:.1} MB)",
        url,
        bytes.len() as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

/// Extrae libpdfium del archivo .tgz al directorio destino.
fn extraer_libreria(
    archivo_tgz: &Path,
    dir_destino: &Path,
    nombre_lib: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let contenido = fs::File::open(archivo_tgz)?;
    let decodificador = GzDecoder::new(io::BufReader::new(contenido));
    let mut archivo_tar = Archive::new(decodificador);

    for entrada in archivo_tar.entries()? {
        let mut entrada = entrada?;
        let ruta = entrada.path()?.to_path_buf();

        let nombre_archivo = ruta.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if nombre_archivo == nombre_lib {
            let ruta_destino = dir_destino.join(nombre_lib);
            let mut archivo_destino = fs::File::create(&ruta_destino)?;
            io::copy(&mut entrada, &mut archivo_destino)?;

            eprintln!(
                "cargo:warning=Extraido {} en {}",
                nombre_lib,
                ruta_destino.display()
            );
            return Ok(ruta_destino);
        }
    }

    Err(format!(
        "{} no encontrado dentro del archivo {}",
        nombre_lib,
        archivo_tgz.display()
    )
    .into())
}

/// Copia la libreria al directorio target/{profile}/ para que el binario la encuentre en runtime.
fn copiar_a_target_profile(ruta_lib: &Path, dir_proyecto: &Path, nombre_lib: &str) {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let binary_dir = dir_proyecto.join("target").join(&profile);
    if binary_dir.exists() {
        if let Err(e) = fs::copy(ruta_lib, binary_dir.join(nombre_lib)) {
            eprintln!(
                "cargo:warning=No se pudo copiar {} a target/{}: {}",
                nombre_lib, profile, e
            );
        }
    }
}

/// Resuelve `pdfium` antes de compilar el crate principal.
///
/// El build script fija una versión conocida de `pdfium-binaries`, reutiliza
/// artefactos ya presentes en el árbol del proyecto y deja la librería junto al
/// ejecutable final para evitar dependencias implícitas del directorio de
/// trabajo desde el que se lance el binario.
fn main() {
    let nombre_lib = obtener_nombre_libreria();
    let dir_proyecto = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ruta_lib_proyecto = dir_proyecto.join(nombre_lib);

    if ruta_lib_proyecto.exists() {
        eprintln!(
            "cargo:warning=Usando {} existente en {}",
            nombre_lib,
            ruta_lib_proyecto.display()
        );
        println!("cargo:rustc-link-search=native={}", dir_proyecto.display());
        println!("cargo:rerun-if-changed={}", ruta_lib_proyecto.display());
        copiar_a_target_profile(&ruta_lib_proyecto, &dir_proyecto, nombre_lib);
        return;
    }

    let nombre_asset = obtener_nombre_asset();
    let url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/download/{}/{}",
        PDFIUM_VERSION, nombre_asset
    );

    let dir_temporal = dir_proyecto.join("target").join("pdfium-download");
    fs::create_dir_all(&dir_temporal).expect("Error creando directorio temporal");

    let ruta_tgz = dir_temporal.join(nombre_asset);

    if !ruta_tgz.exists() {
        if let Err(e) = descargar_pdfium(&url, &ruta_tgz) {
            eprintln!("cargo:warning=FALLO descargando pdfium: {}", e);
            eprintln!("cargo:warning=El soporte PDF no estara disponible.");
            eprintln!(
                "cargo:warning=Para resolver: descargue manualmente {} y coloquelo en {}",
                nombre_lib,
                dir_proyecto.display()
            );
            return;
        }
    }

    match extraer_libreria(&ruta_tgz, &dir_proyecto, nombre_lib) {
        Ok(_ruta) => {
            println!("cargo:rustc-link-search=native={}", dir_proyecto.display());
            println!("cargo:rerun-if-changed={}", ruta_lib_proyecto.display());
            eprintln!("cargo:warning=libpdfium configurado exitosamente (plug and play)");
            copiar_a_target_profile(&ruta_lib_proyecto, &dir_proyecto, nombre_lib);
        }
        Err(e) => {
            eprintln!("cargo:warning=FALLO extrayendo pdfium: {}", e);
            eprintln!(
                "cargo:warning=Descargue manualmente {} y coloquelo en {}",
                nombre_lib,
                dir_proyecto.display()
            );
        }
    }
}
