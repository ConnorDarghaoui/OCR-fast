use crate::domain::{
    AlignmentHint, BlockContent, ElementBlueprint, ElementRole, EmphasisHint, Page, ProcessingMode,
    ResolvedBlock, StyleHints,
};

pub(crate) fn build_element(
    page: &Page,
    resolved: &ResolvedBlock,
    reading_order: u32,
    column_index: u32,
    total_columns: u32,
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
) -> ElementBlueprint {
    let (text, table, image_crop) = match &resolved.content {
        BlockContent::Text(text) => (text.clone(), None, None),
        BlockContent::Table(table) => (table.to_plain_text(), Some(table.clone()), None),
        BlockContent::Image(crop) | BlockContent::Raster(crop) => {
            (String::new(), None, Some(crop.clone()))
        }
        BlockContent::Empty => (String::new(), None, None),
    };

    ElementBlueprint {
        role: resolved.role,
        bounding_box: resolved.detected.bounding_box.clone(),
        reading_order,
        column_index,
        total_columns,
        text,
        ocr_confidence: resolved.ocr_confidence,
        layout_confidence: resolved.detected.layout_confidence,
        suspected_header: false,
        suspected_footer: false,
        table,
        image_crop,
        style: infer_style(
            page,
            resolved,
            total_columns,
            processing_mode,
            spacing_before_pt,
            left_indent_pt,
        ),
    }
}

fn infer_style(
    page: &Page,
    resolved: &ResolvedBlock,
    total_columns: u32,
    processing_mode: ProcessingMode,
    spacing_before_pt: f32,
    left_indent_pt: f32,
) -> StyleHints {
    let page_width = page.dimensions.width.max(1);
    let page_height = page.dimensions.height.max(1);
    let width_ratio = resolved.detected.bounding_box.width as f32 / page_width as f32;
    let height_ratio = resolved.detected.bounding_box.height as f32 / page_height as f32;
    let page_center = page_width as f32 / 2.0;
    let block_center = center_x_resolved(resolved) as f32;
    let centered = (block_center - page_center).abs() <= (page_width as f32 * 0.12);

    let alignment = if width_ratio >= 0.85 {
        AlignmentHint::FullWidth
    } else if matches!(resolved.role, ElementRole::Title) && centered {
        AlignmentHint::Center
    } else if resolved.detected.bounding_box.x >= page_width.saturating_mul(55) / 100
        && width_ratio < 0.40
    {
        AlignmentHint::Right
    } else {
        AlignmentHint::Left
    };

    let emphasis = match resolved.role {
        ElementRole::Title => EmphasisHint::Strong,
        ElementRole::Separator | ElementRole::Stamp => EmphasisHint::Neutral,
        _ => EmphasisHint::Regular,
    };

    let font_scale = match resolved.role {
        ElementRole::Title => (1.2 + height_ratio * 14.0).clamp(1.4, 2.4),
        ElementRole::Paragraph | ElementRole::ListItem => {
            (0.9 + height_ratio * 6.0).clamp(0.9, 1.2)
        }
        ElementRole::Table => 0.95,
        ElementRole::Formula => 1.1,
        _ => 1.0,
    };

    let preserve_positioning = processing_mode == ProcessingMode::VisualPreservation
        || matches!(
            resolved.content,
            BlockContent::Image(_) | BlockContent::Raster(_) | BlockContent::Table(_)
        )
        || matches!(
            resolved.role,
            ElementRole::Figure
                | ElementRole::Formula
                | ElementRole::Signature
                | ElementRole::Stamp
                | ElementRole::Separator
        )
        || total_columns > 1;

    StyleHints {
        alignment,
        emphasis,
        font_scale,
        spacing_before_pt,
        left_indent_pt,
        keep_with_next: matches!(resolved.role, ElementRole::Title | ElementRole::Separator),
        preserve_positioning,
    }
}

fn center_x_resolved(block: &ResolvedBlock) -> u32 {
    block
        .detected
        .bounding_box
        .x
        .saturating_add(block.detected.bounding_box.width / 2)
}
