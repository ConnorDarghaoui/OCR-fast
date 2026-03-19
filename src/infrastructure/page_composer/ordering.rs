use crate::domain::{Block, BlockType, Page};

pub(crate) fn order_blocks_for_document<'a>(blocks: &'a [Block], page_width: u32) -> Vec<&'a Block> {
    let mut sorted: Vec<&Block> = blocks.iter().collect();
    sorted.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x, block.reading_order));

    if estimate_columns(blocks, page_width) == 1 {
        return sorted;
    }

    let mut anchors = Vec::new();
    let mut rest = Vec::new();
    for block in sorted {
        if is_visual_anchor(block, page_width) {
            anchors.push(block);
        } else {
            rest.push(block);
        }
    }

    let mut ordered = Vec::with_capacity(blocks.len());
    let mut segment_start = 0;

    for anchor in anchors {
        let mut segment = Vec::new();
        let mut pending = Vec::new();

        for block in rest {
            if block.bounding_box.y >= segment_start && block.bounding_box.y < anchor.bounding_box.y
            {
                segment.push(block);
            } else {
                pending.push(block);
            }
        }

        ordered.extend(order_segment_by_columns(segment, page_width));
        ordered.push(anchor);
        rest = pending;
        segment_start = anchor
            .bounding_box
            .y
            .saturating_add(anchor.bounding_box.height);
    }

    ordered.extend(order_segment_by_columns(rest, page_width));
    ordered
}

pub(crate) fn order_blocks_for_visual<'a>(blocks: &'a [Block]) -> Vec<&'a Block> {
    let mut sorted: Vec<&Block> = blocks.iter().collect();
    sorted.sort_by_key(|block| (block.reading_order, block.bounding_box.y, block.bounding_box.x));
    sorted
}

pub(crate) fn estimate_columns(blocks: &[Block], page_width: u32) -> u32 {
    let blocks: Vec<&Block> = blocks.iter().collect();
    if segment_looks_two_column(&blocks, page_width) {
        2
    } else {
        1
    }
}

pub(crate) fn infer_column_bases(blocks: &[&Block], page_width: u32) -> [u32; 2] {
    let mut left_base = page_width;
    let mut right_base = page_width;

    for block in blocks {
        if is_visual_anchor(block, page_width) {
            continue;
        }

        if center_x(block) <= page_width / 2 {
            left_base = left_base.min(block.bounding_box.x);
        } else {
            right_base = right_base.min(block.bounding_box.x);
        }
    }

    if left_base == page_width {
        left_base = 0;
    }
    if right_base == page_width {
        right_base = page_width / 2;
    }

    [left_base, right_base]
}

pub(crate) fn infer_spacing_before(
    blocks: &[&Block],
    current_index: usize,
    page_width: u32,
    total_columns: u32,
    column_index: u32,
) -> f32 {
    if current_index == 0 {
        return 0.0;
    }

    let current = blocks[current_index];

    for previous in blocks[..current_index].iter().rev() {
        let previous_columns = if is_visual_anchor(previous, page_width) {
            1
        } else {
            total_columns
        };
        let previous_column = block_column_index(previous, previous_columns, page_width);

        if previous_columns != total_columns || previous_column != column_index {
            continue;
        }

        let previous_bottom = previous
            .bounding_box
            .y
            .saturating_add(previous.bounding_box.height);
        let gap_px = current.bounding_box.y.saturating_sub(previous_bottom);
        return px_to_pt(gap_px).clamp(0.0, 32.0) as f32;
    }

    0.0
}

pub(crate) fn infer_indent_pt(
    block: &Block,
    column_index: u32,
    total_columns: u32,
    column_bases: &[u32; 2],
) -> f32 {
    if total_columns == 1 {
        return 0.0;
    }

    let base_x = column_bases[column_index as usize];
    px_to_pt(block.bounding_box.x.saturating_sub(base_x)) as f32
}

pub(crate) fn reorder_page_document(page: &mut Page) {
    if page.blocks.len() <= 1 {
        renumber_reading_order(&mut page.blocks);
        return;
    }

    let ordered = order_blocks_for_document(&page.blocks, page.dimensions.width);
    page.blocks = ordered.into_iter().cloned().collect();
    renumber_reading_order(&mut page.blocks);
}

pub(crate) fn preserve_page_visual_order(page: &mut Page) {
    page.blocks.sort_by_key(|block| (block.reading_order, block.bounding_box.y, block.bounding_box.x));
    renumber_reading_order(&mut page.blocks);
}

pub(crate) fn is_visual_anchor(block: &Block, page_width: u32) -> bool {
    if matches!(block.block_type, BlockType::Title | BlockType::Separator) {
        return true;
    }

    let width_ratio = block.bounding_box.width as f32 / page_width.max(1) as f32;
    width_ratio >= 0.70
}

pub(crate) fn center_x(block: &Block) -> u32 {
    block
        .bounding_box
        .x
        .saturating_add(block.bounding_box.width / 2)
}

pub(crate) fn px_to_pt(px: u32) -> f64 {
    (px as f64) * (72.0 / 150.0)
}

fn order_segment_by_columns<'a>(blocks: Vec<&'a Block>, page_width: u32) -> Vec<&'a Block> {
    if !segment_looks_two_column(&blocks, page_width) {
        let mut linear = blocks;
        linear.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
        return linear;
    }

    let mut left = Vec::new();
    let mut right = Vec::new();
    let page_center = page_width / 2;

    for block in blocks {
        if center_x(block) <= page_center {
            left.push(block);
        } else {
            right.push(block);
        }
    }

    left.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
    right.sort_by_key(|block| (block.bounding_box.y, block.bounding_box.x));
    left.extend(right);
    left
}

fn segment_looks_two_column(blocks: &[&Block], page_width: u32) -> bool {
    let page_width = page_width.max(1);
    let mut left = 0usize;
    let mut right = 0usize;
    let mut min_right_x = page_width;
    let mut max_left_x = 0u32;
    let mut top_left = u32::MAX;
    let mut top_right = u32::MAX;

    for block in blocks {
        if is_visual_anchor(block, page_width) {
            continue;
        }

        if !matches!(
            block.block_type,
            BlockType::Text | BlockType::List | BlockType::Table | BlockType::Formula
        ) {
            continue;
        }

        let center = center_x(block);
        if center <= page_width / 2 {
            left += 1;
            max_left_x = max_left_x.max(
                block
                    .bounding_box
                    .x
                    .saturating_add(block.bounding_box.width),
            );
            top_left = top_left.min(block.bounding_box.y);
        } else {
            right += 1;
            min_right_x = min_right_x.min(block.bounding_box.x);
            top_right = top_right.min(block.bounding_box.y);
        }
    }

    if left < 2 || right < 2 {
        return false;
    }

    let min_gutter = page_width.saturating_mul(4) / 100;
    let gutter = min_right_x.saturating_sub(max_left_x);
    let top_delta = top_left.abs_diff(top_right);
    gutter >= min_gutter && top_delta <= 220
}

fn block_column_index(block: &Block, total_columns: u32, page_width: u32) -> u32 {
    if total_columns == 2 && center_x(block) > page_width / 2 {
        1
    } else {
        0
    }
}

fn renumber_reading_order(blocks: &mut [Block]) {
    for (index, block) in blocks.iter_mut().enumerate() {
        block.reading_order = index as u32;
    }
}
