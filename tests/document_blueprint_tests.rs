use ocrfast::application::pipeline::OcrPipeline;
use ocrfast::domain::{
    Block, BlockType, Dimensions, Document, ElementRole, Page, ProcessingProfile, Rectangle,
};
use ocrfast::infrastructure::document_blueprints::HighFidelityBlueprintBuilder;
use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
use ocrfast::infrastructure::ocr_engines::stub::StubOcrEngine;
use ocrfast::interfaces::ports::DocumentBlueprintBuilderPort;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn bloque(
    block_type: BlockType,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    reading_order: u32,
    content: &str,
) -> Block {
    Block {
        block_type,
        bounding_box: Rectangle {
            x,
            y,
            width,
            height,
        },
        content: content.to_string(),
        confidence: 0.98,
        embedded_image: None,
        table_structure: None,
        reading_order,
    }
}

fn documento_dos_columnas() -> Document {
    Document {
        id: "doc-blueprint".to_string(),
        source_path: PathBuf::from("/tmp/libro-fotocopiado.png"),
        pages: vec![Page {
            number: 1,
            dimensions: Dimensions {
                width: 1200,
                height: 1800,
            },
            blocks: vec![
                bloque(BlockType::Title, 120, 80, 960, 120, 0, "Capitulo 1"),
                bloque(BlockType::Text, 120, 280, 420, 180, 1, "izquierda superior"),
                bloque(BlockType::Text, 120, 520, 420, 180, 2, "izquierda inferior"),
                bloque(BlockType::Text, 660, 300, 420, 180, 3, "derecha superior"),
                bloque(BlockType::Text, 660, 540, 420, 180, 4, "derecha inferior"),
                bloque(BlockType::Image, 150, 800, 380, 260, 5, ""),
            ],
            image_data: None,
        }],
        metadata: HashMap::new(),
    }
}

#[test]
fn test_blueprint_detecta_columnas_y_preserva_imagenes() {
    let builder = HighFidelityBlueprintBuilder::new();
    let blueprint = builder
        .build_blueprint(&documento_dos_columnas())
        .expect("El builder debe producir blueprint");

    let pagina = &blueprint.pages[0];
    assert_eq!(pagina.elements[0].role, ElementRole::Title);
    assert_eq!(pagina.elements[0].total_columns, 1);
    assert!(pagina.elements[0].style.keep_with_next);

    let parrafos: Vec<_> = pagina
        .elements
        .iter()
        .filter(|elemento| elemento.role == ElementRole::Paragraph)
        .map(|elemento| {
            (
                elemento.text.as_str(),
                elemento.column_index,
                elemento.total_columns,
            )
        })
        .collect();

    assert_eq!(
        parrafos,
        vec![
            ("izquierda superior", 0, 2),
            ("izquierda inferior", 0, 2),
            ("derecha superior", 1, 2),
            ("derecha inferior", 1, 2),
        ]
    );

    let figura = pagina
        .elements
        .iter()
        .find(|elemento| elemento.role == ElementRole::Figure)
        .expect("Debe existir un bloque de figura");
    let segundo_parrafo = pagina
        .elements
        .iter()
        .find(|elemento| elemento.text == "izquierda inferior")
        .expect("Debe existir el segundo párrafo");
    assert!(
        segundo_parrafo.style.spacing_before_pt > 0.0,
        "El blueprint debe inferir separación vertical entre párrafos consecutivos"
    );

    let recorte = figura
        .image_crop
        .as_ref()
        .expect("La figura debe conservar referencia de recorte");
    assert_eq!(recorte.page_number, 1);
    assert_eq!(recorte.bounding_box.x, 150);
    assert_eq!(recorte.bounding_box.width, 380);
}

#[test]
fn test_pipeline_retorna_blueprint_sin_romper_documento_clasico() {
    let pipeline = OcrPipeline::new(
        Arc::new(StubDocumentParser::new()),
        Arc::new(StubOcrEngine::new()),
    )
    .with_blueprint_builder(Arc::new(HighFidelityBlueprintBuilder::new()));

    let resultado = pipeline
        .procesar_documento_con_blueprint(
            Path::new("/tmp/ocrfast-blueprint.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        )
        .expect("El pipeline enriquecido debe completarse");

    assert!(
        !resultado.document.pages.is_empty(),
        "El documento operativo no debe perderse"
    );

    let blueprint = resultado
        .blueprint
        .expect("La ejecución con builder debe producir blueprint");

    assert_eq!(blueprint.document_id, resultado.document.id);
    assert_eq!(blueprint.pages.len(), resultado.document.pages.len());
    assert_eq!(
        blueprint.pages[0].elements.len(),
        resultado.document.pages[0].blocks.len()
    );
    assert_eq!(blueprint.pages[0].elements[0].role, ElementRole::Title);
}
