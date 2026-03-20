use ocrfast::application::pipeline::OcrPipeline;
use ocrfast::domain::{
    BlockType, Document, Job, JobStatus, OutputFormat, ProcessingMode, ProcessingModePreference,
    ProcessingProfile, DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE,
};
use ocrfast::infrastructure::automata::BlockAutomata;
use ocrfast::infrastructure::document_parsers::image_parser::ImageDocumentParser;
use ocrfast::infrastructure::exporters::DefaultJobExporter;
use ocrfast::infrastructure::ocr_engines::onnx::{
    engine::OnnxOcrEngine, model_downloader::ModelDownloader,
    runtime_provisioner::ModelRuntimeProvisioner,
};
use ocrfast::infrastructure::page_composer::PageComposer;
use ocrfast::infrastructure::postprocessors::TextPostprocessor;
use ocrfast::interfaces::ports::JobExporterPort;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

const DEFAULT_MANIFEST_PATH: &str = "tests/fixtures/real/local/manifest.json";
const DEFAULT_OUTPUT_DIR: &str = "tests/fixtures/real/output";
const DEFAULT_DIRECTORY_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tif", "tiff"];

#[derive(Debug, Deserialize)]
struct CorpusManifest {
    name: String,
    cases: Vec<CorpusCase>,
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    kind: CorpusCaseKind,
    input: CorpusInput,
    #[serde(default)]
    profile: ManifestProfile,
    #[serde(default)]
    mode: ManifestMode,
    #[serde(default)]
    export_formats: Vec<ManifestOutputFormat>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CorpusCaseKind {
    PhotoBook,
    DocumentScan,
    Screenshot,
    Hybrid,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CorpusInput {
    File {
        path: String,
    },
    Directory {
        path: String,
        #[serde(default)]
        extensions: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManifestProfile {
    Fast,
    Balanced,
    Accurate,
}

impl Default for ManifestProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

impl From<ManifestProfile> for ProcessingProfile {
    fn from(value: ManifestProfile) -> Self {
        match value {
            ManifestProfile::Fast => ProcessingProfile::Fast,
            ManifestProfile::Balanced => ProcessingProfile::Balanced,
            ManifestProfile::Accurate => ProcessingProfile::Accurate,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManifestMode {
    Auto,
    Document,
    Visual,
}

impl Default for ManifestMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl From<ManifestMode> for ProcessingModePreference {
    fn from(value: ManifestMode) -> Self {
        match value {
            ManifestMode::Auto => ProcessingModePreference::Auto,
            ManifestMode::Document => ProcessingModePreference::DocumentReconstruction,
            ManifestMode::Visual => ProcessingModePreference::VisualPreservation,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManifestOutputFormat {
    Txt,
    Latex,
    Pdf,
    Json,
}

impl From<ManifestOutputFormat> for OutputFormat {
    fn from(value: ManifestOutputFormat) -> Self {
        match value {
            ManifestOutputFormat::Txt => OutputFormat::Txt,
            ManifestOutputFormat::Latex => OutputFormat::Latex,
            ManifestOutputFormat::Pdf => OutputFormat::Pdf,
            ManifestOutputFormat::Json => OutputFormat::Json,
        }
    }
}

#[derive(Debug, Serialize)]
struct CorpusReport {
    manifest_name: String,
    generated_at_utc: String,
    cases: Vec<CorpusCaseReport>,
}

#[derive(Debug, Default, Serialize)]
struct CorpusCaseReport {
    id: String,
    kind: String,
    profile: String,
    mode_preference: String,
    inputs_processed: usize,
    total_pages: usize,
    total_blocks: usize,
    text_like_blocks: usize,
    table_blocks: usize,
    image_like_blocks: usize,
    fallback_blocks: usize,
    visual_pages: usize,
    document_pages: usize,
    avg_ocr_confidence: Option<f32>,
    elapsed_ms: u128,
    exported_artifacts: Vec<String>,
    notes: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = env_logger::try_init();

    let args: Vec<String> = env::args().collect();
    let manifest_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MANIFEST_PATH));
    let output_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_DIR));

    if !manifest_path.exists() {
        return Err(format!(
            "No se encontro el manifest {:?}. Copia tests/fixtures/real/manifest.example.json a tests/fixtures/real/local/manifest.json y ajusta las rutas.",
            manifest_path
        )
        .into());
    }

    fs::create_dir_all(&output_dir)?;

    let manifest_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest: CorpusManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;

    let pipeline = construir_pipeline()?;
    let exporter = DefaultJobExporter::new();
    let automata = BlockAutomata::new();
    let composer = PageComposer::new();
    let mut reports = Vec::with_capacity(manifest.cases.len());

    for case in &manifest.cases {
        let case_output_dir = output_dir.join(&case.id);
        fs::create_dir_all(&case_output_dir)?;
        reports.push(procesar_caso(
            case,
            &manifest_dir,
            &case_output_dir,
            &pipeline,
            &exporter,
            &automata,
            &composer,
        )?);
    }

    let report = CorpusReport {
        manifest_name: manifest.name,
        generated_at_utc: chrono::Utc::now().to_rfc3339(),
        cases: reports,
    };
    let report_path = output_dir.join("corpus-report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;

    println!("Reporte escrito en {}", report_path.display());
    for case in &report.cases {
        println!(
            "- {}: {} entradas, {} paginas, {} bloques, {} fallbacks, {} ms",
            case.id,
            case.inputs_processed,
            case.total_pages,
            case.total_blocks,
            case.fallback_blocks,
            case.elapsed_ms
        );
    }

    Ok(())
}

fn construir_pipeline() -> Result<OcrPipeline, Box<dyn std::error::Error>> {
    let downloader = ModelDownloader::new()?;
    let directorio_modelos = downloader.directorio_base().to_path_buf();
    let provisioner = ModelRuntimeProvisioner::with_downloader(downloader);
    let runtime = provisioner.provision(None, None, None)?;

    let motor_ocr = Arc::new(
        OnnxOcrEngine::from_provisioned_runtime(&runtime)
            .or_else(|_| OnnxOcrEngine::from_directory(&directorio_modelos))?,
    );
    let parser = Arc::new(ImageDocumentParser::new());
    let postprocesador = Arc::new(TextPostprocessor::new());

    Ok(OcrPipeline::new(parser, motor_ocr).with_postprocessor(postprocesador))
}

fn procesar_caso(
    case: &CorpusCase,
    manifest_dir: &Path,
    case_output_dir: &Path,
    pipeline: &OcrPipeline,
    exporter: &DefaultJobExporter,
    automata: &BlockAutomata,
    composer: &PageComposer,
) -> Result<CorpusCaseReport, Box<dyn std::error::Error>> {
    let entradas = resolver_entradas(case, manifest_dir)?;
    if entradas.is_empty() {
        return Err(format!("El caso {} no resolvio entradas", case.id).into());
    }

    let inicio = Instant::now();
    let modo_preferencia: ProcessingModePreference = case.mode.into();
    let perfil: ProcessingProfile = case.profile.into();
    let mut aggregate = Document {
        id: case.id.clone(),
        source_path: entrada_raiz(case, manifest_dir),
        pages: Vec::new(),
        metadata: HashMap::new(),
    };
    if modo_preferencia != ProcessingModePreference::Auto {
        aggregate.metadata.insert(
            DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE.to_string(),
            modo_preferencia.metadata_value().to_string(),
        );
    }

    let mut report = CorpusCaseReport {
        id: case.id.clone(),
        kind: format!("{:?}", case.kind),
        profile: format!("{:?}", perfil),
        mode_preference: modo_preferencia.nombre().to_string(),
        inputs_processed: entradas.len(),
        notes: case.notes.clone(),
        ..Default::default()
    };

    let mut total_confidence = 0.0f32;
    let mut confidence_count = 0usize;

    for entrada in entradas {
        let cancelacion = Arc::new(AtomicBool::new(false));
        let mut documento =
            pipeline.procesar_documento(&entrada, &perfil, None, Some(&cancelacion))?;

        if modo_preferencia != ProcessingModePreference::Auto {
            documento.metadata.insert(
                DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE.to_string(),
                modo_preferencia.metadata_value().to_string(),
            );
        }

        for page in &documento.pages {
            let resolved = automata.resolve_page(page);
            report.fallback_blocks += resolved.iter().filter(|block| block.fallback_used).count();
        }

        for page in &documento.pages {
            report.total_pages += 1;
            report.total_blocks += page.blocks.len();

            for block in &page.blocks {
                match block.block_type {
                    BlockType::Text | BlockType::Title | BlockType::List | BlockType::Formula => {
                        report.text_like_blocks += 1;
                    }
                    BlockType::Table => report.table_blocks += 1,
                    BlockType::Image | BlockType::Signature | BlockType::Stamp => {
                        report.image_like_blocks += 1;
                    }
                    _ => {}
                }

                if !block.content.trim().is_empty() {
                    total_confidence += block.confidence as f32;
                    confidence_count += 1;
                }
            }
        }

        anexar_paginas(&mut aggregate, documento);
    }

    let blueprint = composer.compose(&aggregate)?;
    report.visual_pages = blueprint
        .pages
        .iter()
        .filter(|page| page.processing_mode == ProcessingMode::VisualPreservation)
        .count();
    report.document_pages = blueprint
        .pages
        .iter()
        .filter(|page| page.processing_mode == ProcessingMode::DocumentReconstruction)
        .count();
    report.avg_ocr_confidence = if confidence_count > 0 {
        Some(total_confidence / confidence_count as f32)
    } else {
        None
    };

    let job = Job {
        id: format!("corpus-{}", case.id),
        document: aggregate,
        status: JobStatus::Completed,
        created_at: std::time::SystemTime::now(),
        completed_at: Some(std::time::SystemTime::now()),
        profile: perfil,
        error_message: None,
        formato_salida: OutputFormat::Json,
        modo_procesamiento: modo_preferencia,
    };

    for formato in &case.export_formats {
        let output_format: OutputFormat = (*formato).into();
        let mut export_job = job.clone();
        export_job.formato_salida = output_format;
        let artifact_path =
            case_output_dir.join(format!("{}.{}", case.id, output_format.extension()));
        exporter.export_job(&export_job, &artifact_path)?;
        report
            .exported_artifacts
            .push(artifact_path.to_string_lossy().into_owned());
    }

    report.elapsed_ms = inicio.elapsed().as_millis();
    Ok(report)
}

fn resolver_entradas(
    case: &CorpusCase,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    match &case.input {
        CorpusInput::File { path } => Ok(vec![resolver_path(manifest_dir, path)]),
        CorpusInput::Directory { path, extensions } => {
            let directorio = resolver_path(manifest_dir, path);
            if !directorio.is_dir() {
                return Err(format!("La entrada {:?} no es un directorio", directorio).into());
            }

            let extensiones = if extensions.is_empty() {
                DEFAULT_DIRECTORY_EXTENSIONS
                    .iter()
                    .map(|ext| ext.to_string())
                    .collect::<Vec<_>>()
            } else {
                extensions.iter().map(|ext| ext.to_lowercase()).collect()
            };

            let mut entradas = fs::read_dir(&directorio)?
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| {
                                extensiones
                                    .iter()
                                    .any(|allowed| allowed == &ext.to_lowercase())
                            })
                            .unwrap_or(false)
                })
                .collect::<Vec<_>>();
            entradas.sort();
            Ok(entradas)
        }
    }
}

fn resolver_path(base: &Path, candidate: &str) -> PathBuf {
    let path = PathBuf::from(candidate);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn entrada_raiz(case: &CorpusCase, manifest_dir: &Path) -> PathBuf {
    match &case.input {
        CorpusInput::File { path } => resolver_path(manifest_dir, path),
        CorpusInput::Directory { path, .. } => resolver_path(manifest_dir, path),
    }
}

fn anexar_paginas(aggregate: &mut Document, mut documento: Document) {
    let pagina_base = aggregate.pages.len() as u32;
    for (offset, page) in documento.pages.iter_mut().enumerate() {
        page.number = pagina_base + offset as u32 + 1;
    }
    aggregate.pages.extend(documento.pages);
}
