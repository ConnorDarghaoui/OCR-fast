use ocrfast::application::pipeline::OcrPipeline;
use ocrfast::domain::{
    Block, BlockType, Dimensions, Document, ElementRole, Page, ProcessingMode, ProcessingProfile,
    Rectangle, DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE,
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
        layout_confidence: Some(0.91),
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

fn documento_con_header_footer_repetido() -> Document {
    Document {
        id: "doc-header-footer".to_string(),
        source_path: PathBuf::from("/tmp/libro-header-footer.pdf"),
        pages: vec![
            Page {
                number: 1,
                dimensions: Dimensions {
                    width: 1200,
                    height: 1800,
                },
                blocks: vec![
                    bloque(BlockType::Text, 110, 55, 980, 55, 0, "Historia Universal"),
                    bloque(
                        BlockType::Text,
                        110,
                        210,
                        900,
                        180,
                        1,
                        "contenido pagina uno",
                    ),
                    bloque(BlockType::Text, 110, 1650, 980, 45, 2, "Pagina 1"),
                ],
                image_data: None,
            },
            Page {
                number: 2,
                dimensions: Dimensions {
                    width: 1200,
                    height: 1800,
                },
                blocks: vec![
                    bloque(BlockType::Text, 112, 58, 978, 55, 0, "Historia Universal"),
                    bloque(
                        BlockType::Text,
                        110,
                        195,
                        900,
                        180,
                        1,
                        "contenido pagina dos distinto",
                    ),
                    bloque(BlockType::Text, 108, 1652, 982, 45, 2, "Pagina 2"),
                ],
                image_data: None,
            },
        ],
        metadata: HashMap::new(),
    }
}

fn documento_captura_marketplace() -> Document {
    Document {
        id: "doc-captura-marketplace".to_string(),
        source_path: PathBuf::from("/tmp/captura-ebay.png"),
        pages: vec![Page {
            number: 1,
            dimensions: Dimensions {
                width: 1440,
                height: 1800,
            },
            blocks: vec![
                bloque(BlockType::Image, 60, 220, 620, 680, 0, ""),
                bloque(
                    BlockType::Text,
                    760,
                    250,
                    420,
                    70,
                    1,
                    "Vintage camera listing",
                ),
                bloque(BlockType::Text, 760, 370, 500, 90, 2, "$129.99 Buy it now"),
                bloque(BlockType::Text, 760, 520, 420, 120, 3, "Ships from Panama"),
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
    assert_eq!(pagina.elements[0].ocr_confidence, Some(0.98));
    assert_eq!(pagina.elements[0].layout_confidence, Some(0.91));
    assert!(!pagina.elements[0].suspected_header);
    assert!(!pagina.elements[0].suspected_footer);

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
fn test_blueprint_marca_hints_conservadores_para_header_y_footer() {
    let builder = HighFidelityBlueprintBuilder::new();
    let blueprint = builder
        .build_blueprint(&documento_con_header_footer_repetido())
        .expect("El builder debe producir blueprint con hints");

    let pagina_uno = &blueprint.pages[0];
    let pagina_dos = &blueprint.pages[1];

    assert!(pagina_uno.elements[0].suspected_header);
    assert!(pagina_dos.elements[0].suspected_header);
    assert!(pagina_uno.elements[2].suspected_footer);
    assert!(pagina_dos.elements[2].suspected_footer);

    assert!(!pagina_uno.elements[1].suspected_header);
    assert!(!pagina_uno.elements[1].suspected_footer);
    assert!(!pagina_dos.elements[1].suspected_header);
    assert!(!pagina_dos.elements[1].suspected_footer);
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
    assert_eq!(blueprint.pages[0].elements[0].ocr_confidence, Some(0.90));
    assert_eq!(blueprint.pages[0].elements[0].layout_confidence, None);
}

#[test]
fn test_blueprint_detecta_captura_visual_y_evita_reflujo_documental() {
    let builder = HighFidelityBlueprintBuilder::new();
    let blueprint = builder
        .build_blueprint(&documento_captura_marketplace())
        .expect("El builder debe clasificar capturas visuales");

    assert_eq!(
        blueprint.processing_mode,
        ProcessingMode::VisualPreservation
    );
    assert!(
        blueprint.pages[0]
            .elements
            .iter()
            .all(|elemento| elemento.total_columns == 1),
        "Las capturas visuales no deben activar reflujo por columnas"
    );
    assert!(
        blueprint.pages[0]
            .elements
            .iter()
            .all(|elemento| elemento.style.preserve_positioning),
        "Las capturas visuales deben preservar posicionamiento"
    );
    assert!(
        !blueprint.pages[0]
            .elements
            .iter()
            .any(|elemento| elemento.suspected_header || elemento.suspected_footer),
        "Las capturas de interfaz no deben pasar por hints documentales de header/footer"
    );
}

#[test]
fn test_blueprint_honra_override_manual_documental_desde_metadata() {
    let mut documento = documento_captura_marketplace();
    documento.metadata.insert(
        DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE.to_string(),
        "document".to_string(),
    );

    let blueprint = HighFidelityBlueprintBuilder::new()
        .build_blueprint(&documento)
        .expect("El builder debe honrar la preferencia manual");

    assert_eq!(
        blueprint.processing_mode,
        ProcessingMode::DocumentReconstruction
    );
}

#[test]
fn test_blueprint_honra_override_manual_visual_desde_metadata() {
    let mut documento = documento_dos_columnas();
    documento.metadata.insert(
        DOCUMENT_METADATA_PROCESSING_MODE_PREFERENCE.to_string(),
        "visual".to_string(),
    );

    let blueprint = HighFidelityBlueprintBuilder::new()
        .build_blueprint(&documento)
        .expect("El builder debe honrar la preservacion visual forzada");

    assert_eq!(
        blueprint.processing_mode,
        ProcessingMode::VisualPreservation
    );
    assert!(blueprint.pages[0]
        .elements
        .iter()
        .all(|elemento| elemento.total_columns == 1));
}
