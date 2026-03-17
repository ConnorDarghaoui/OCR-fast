use crate::domain::errors::DocumentError;
use crate::domain::{Block, BlockType, Dimensions, Document, Page, Rectangle};
use crate::interfaces::ports::DocumentParserPort;
use std::collections::HashMap;
use std::path::Path;

/// Parser stub para pruebas de UI y pipeline sin I/O real.
///
/// El objetivo de este adaptador no es emular fidelidad documental, sino proveer
/// una estructura suficientemente estable para validar navegación, persistencia y
/// render sin depender de archivos ni librerías nativas.
pub struct StubDocumentParser;

impl StubDocumentParser {
    /// Construye un parser stub sin estado interno.
    pub fn new() -> Self {
        Self
    }
}

impl DocumentParserPort for StubDocumentParser {
    fn parse(&self, path: &Path) -> Result<Document, DocumentError> {
        log::info!("StubDocumentParser: parseando {:?}", path);

        let mut metadata = HashMap::new();
        metadata.insert("source_format".to_string(), "stub".to_string());
        metadata.insert(
            "filename".to_string(),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
        );

        Ok(Document {
            id: uuid::Uuid::new_v4().to_string(),
            source_path: path.to_path_buf(),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 2480,
                    height: 3508,
                },
                blocks: vec![
                    Block {
                        block_type: BlockType::Title,
                        bounding_box: Rectangle {
                            x: 100,
                            y: 100,
                            width: 2280,
                            height: 80,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    },
                    Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 100,
                            y: 250,
                            width: 2280,
                            height: 400,
                        },
                        content: String::new(),
                        confidence: 0.0,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 1,
                    },
                ],
                image_data: None,
            }],
            metadata,
        })
    }
}
