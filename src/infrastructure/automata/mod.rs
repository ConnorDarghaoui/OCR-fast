use crate::domain::{
    Block, BlockContent, BlockType, DetectedBlock, ElementRole, ImageCropRef, Page, ResolvedBlock,
};

/// Umbral conservador para degradar bloques OCR débiles a raster.
const BLOCK_OCR_CONFIDENCE_THRESHOLD: f32 = 0.74;

/// Estrategia para resolver un bloque detectado a contenido final.
///
/// Cada estrategia recibe el bloque ya detectado y la página de origen, y debe
/// devolver un `ResolvedBlock` sin reordenar la geometría ni mutar estado
/// compartido. El fallback a raster se gestiona dentro del autómata.
pub trait BlockStrategy: Send + Sync {
    /// Resuelve un bloque individual y retorna contenido más confianza OCR.
    fn resolve(&self, page: &Page, block: &Block) -> ResolvedBlock;
}

/// Estrategia para bloques textuales y fórmulas simples.
pub struct TextBlockStrategy {
    threshold: f32,
}

impl TextBlockStrategy {
    /// Construye la estrategia textual con umbral explícito de aceptación OCR.
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl Default for TextBlockStrategy {
    fn default() -> Self {
        Self::new(BLOCK_OCR_CONFIDENCE_THRESHOLD)
    }
}

impl BlockStrategy for TextBlockStrategy {
    fn resolve(&self, page: &Page, block: &Block) -> ResolvedBlock {
        let detected = detected_from_block(block);
        let role = map_role(block.block_type);
        let confidence = Some(block.confidence as f32);
        let text = block.content.trim();

        if text.is_empty() || confidence.unwrap_or_default() < self.threshold {
            return ResolvedBlock {
                detected,
                role,
                content: fallback_crop(page.number, block),
                ocr_confidence: confidence,
                fallback_used: true,
            };
        }

        ResolvedBlock {
            detected,
            role,
            content: BlockContent::Text(text.to_string()),
            ocr_confidence: confidence,
            fallback_used: false,
        }
    }
}

/// Estrategia para imágenes, firmas y sellos preservados.
pub struct ImageBlockStrategy;

impl ImageBlockStrategy {
    /// Construye la estrategia de preservación visual sin configuración extra.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImageBlockStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockStrategy for ImageBlockStrategy {
    fn resolve(&self, page: &Page, block: &Block) -> ResolvedBlock {
        ResolvedBlock {
            detected: detected_from_block(block),
            role: map_role(block.block_type),
            content: BlockContent::Image(ImageCropRef {
                page_number: page.number,
                bounding_box: block.bounding_box.clone(),
            }),
            ocr_confidence: Some(block.confidence as f32),
            fallback_used: false,
        }
    }
}

/// Estrategia para bloques tabulares estructurados.
pub struct TableBlockStrategy {
    threshold: f32,
}

impl TableBlockStrategy {
    /// Construye la estrategia tabular con umbral explícito de aceptación OCR.
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl Default for TableBlockStrategy {
    fn default() -> Self {
        Self::new(BLOCK_OCR_CONFIDENCE_THRESHOLD)
    }
}

impl BlockStrategy for TableBlockStrategy {
    fn resolve(&self, page: &Page, block: &Block) -> ResolvedBlock {
        let detected = detected_from_block(block);
        let confidence = Some(block.confidence as f32);
        let role = map_role(block.block_type);

        if let Some(table) = block.table_structure.clone() {
            if confidence.unwrap_or_default() >= self.threshold {
                return ResolvedBlock {
                    detected,
                    role,
                    content: BlockContent::Table(table),
                    ocr_confidence: confidence,
                    fallback_used: false,
                };
            }
        }

        let text = block.content.trim();
        if !text.is_empty() && confidence.unwrap_or_default() >= self.threshold {
            return ResolvedBlock {
                detected,
                role,
                content: BlockContent::Text(text.to_string()),
                ocr_confidence: confidence,
                fallback_used: false,
            };
        }

        ResolvedBlock {
            detected,
            role,
            content: fallback_crop(page.number, block),
            ocr_confidence: confidence,
            fallback_used: true,
        }
    }
}

/// Autómata determinista por bloque.
///
/// Su función es mantener una única decisión por bloque: procesar con la
/// estrategia apropiada o degradar a raster si el resultado no es suficientemente
/// confiable. No reordena páginas ni decide composición global.
pub struct BlockAutomata {
    text_strategy: TextBlockStrategy,
    image_strategy: ImageBlockStrategy,
    table_strategy: TableBlockStrategy,
}

impl BlockAutomata {
    /// Construye el autómata con las estrategias por defecto del producto.
    pub fn new() -> Self {
        Self {
            text_strategy: TextBlockStrategy::default(),
            image_strategy: ImageBlockStrategy::default(),
            table_strategy: TableBlockStrategy::default(),
        }
    }

    /// Resuelve todos los bloques de una página con una decisión terminal por bloque.
    pub fn resolve_page(&self, page: &Page) -> Vec<ResolvedBlock> {
        page.blocks
            .iter()
            .map(|block| self.resolve_block(page, block))
            .collect()
    }

    /// Resuelve un bloque individual usando la estrategia adecuada o fallback raster.
    pub fn resolve_block(&self, page: &Page, block: &Block) -> ResolvedBlock {
        match block.block_type {
            BlockType::Text | BlockType::Title | BlockType::List | BlockType::Formula => {
                self.text_strategy.resolve(page, block)
            }
            BlockType::Table => self.table_strategy.resolve(page, block),
            BlockType::Image | BlockType::Signature | BlockType::Stamp => {
                self.image_strategy.resolve(page, block)
            }
            BlockType::Separator | BlockType::Unknown => ResolvedBlock {
                detected: detected_from_block(block),
                role: map_role(block.block_type),
                content: BlockContent::Empty,
                ocr_confidence: Some(block.confidence as f32),
                fallback_used: false,
            },
        }
    }
}

impl Default for BlockAutomata {
    fn default() -> Self {
        Self::new()
    }
}

fn detected_from_block(block: &Block) -> DetectedBlock {
    DetectedBlock {
        block_type: block.block_type,
        bounding_box: block.bounding_box.clone(),
        reading_order: block.reading_order,
        layout_confidence: block.layout_confidence.map(|value| value as f32),
    }
}

fn fallback_crop(page_number: u32, block: &Block) -> BlockContent {
    BlockContent::Raster(ImageCropRef {
        page_number,
        bounding_box: block.bounding_box.clone(),
    })
}

fn map_role(tipo: BlockType) -> ElementRole {
    match tipo {
        BlockType::Title => ElementRole::Title,
        BlockType::Text => ElementRole::Paragraph,
        BlockType::Table => ElementRole::Table,
        BlockType::Image => ElementRole::Figure,
        BlockType::Formula => ElementRole::Formula,
        BlockType::List => ElementRole::ListItem,
        BlockType::Signature => ElementRole::Signature,
        BlockType::Stamp => ElementRole::Stamp,
        BlockType::Separator => ElementRole::Separator,
        BlockType::Unknown => ElementRole::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Dimensions, Rectangle};

    fn page_with_block(block: Block) -> Page {
        Page {
            number: 1,
            dimensions: Dimensions {
                width: 1000,
                height: 1400,
            },
            blocks: vec![block],
            image_data: Some(vec![1, 2, 3]),
        }
    }

    #[test]
    fn text_strategy_falls_back_to_raster_when_confidence_is_low() {
        let automata = BlockAutomata::new();
        let page = page_with_block(Block {
            block_type: BlockType::Text,
            bounding_box: Rectangle {
                x: 10,
                y: 20,
                width: 200,
                height: 40,
            },
            content: "texto dudoso".to_string(),
            confidence: 0.42,
            layout_confidence: Some(0.91),
            embedded_image: None,
            table_structure: None,
            reading_order: 0,
        });

        let resolved = automata.resolve_block(&page, &page.blocks[0]);

        assert!(matches!(resolved.content, BlockContent::Raster(_)));
        assert!(resolved.fallback_used);
    }

    #[test]
    fn image_strategy_preserves_image_crop() {
        let automata = BlockAutomata::new();
        let page = page_with_block(Block {
            block_type: BlockType::Image,
            bounding_box: Rectangle {
                x: 50,
                y: 80,
                width: 320,
                height: 180,
            },
            content: String::new(),
            confidence: 0.99,
            layout_confidence: Some(0.88),
            embedded_image: None,
            table_structure: None,
            reading_order: 1,
        });

        let resolved = automata.resolve_block(&page, &page.blocks[0]);

        match resolved.content {
            BlockContent::Image(crop) => {
                assert_eq!(crop.page_number, 1);
                assert_eq!(crop.bounding_box.width, 320);
            }
            _ => panic!("El bloque de imagen debe preservarse como recorte"),
        }
    }
}
