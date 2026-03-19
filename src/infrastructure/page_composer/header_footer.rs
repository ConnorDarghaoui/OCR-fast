use crate::domain::{ElementBlueprint, ElementRole, PageBlueprint, ProcessingMode};

pub(crate) fn marcar_hints_encabezado_pie(pages: &mut [PageBlueprint]) {
    if pages.len() < 2 {
        return;
    }

    for page_index in 0..pages.len() {
        let (previous_pages, current_and_rest) = pages.split_at_mut(page_index);
        let Some((current_page, next_pages)) = current_and_rest.split_first_mut() else {
            continue;
        };

        if current_page.processing_mode != ProcessingMode::DocumentReconstruction {
            continue;
        }

        let previous_page = previous_pages
            .iter()
            .rev()
            .find(|page| page.processing_mode == ProcessingMode::DocumentReconstruction);
        let next_page = next_pages
            .iter()
            .find(|page| page.processing_mode == ProcessingMode::DocumentReconstruction);
        let height = current_page.dimensions.height.max(1);

        for element_index in 0..current_page.elements.len() {
            let element = &current_page.elements[element_index];
            let is_header = is_header_footer_candidate(element, height, true)
                && (previous_page
                    .is_some_and(|neighbor| has_repeated_match(element, neighbor, true))
                    || next_page
                        .is_some_and(|neighbor| has_repeated_match(element, neighbor, true)));
            let is_footer = is_header_footer_candidate(element, height, false)
                && (previous_page
                    .is_some_and(|neighbor| has_repeated_match(element, neighbor, false))
                    || next_page
                        .is_some_and(|neighbor| has_repeated_match(element, neighbor, false)));

            let element_mut = &mut current_page.elements[element_index];
            element_mut.suspected_header = is_header;
            element_mut.suspected_footer = is_footer;
        }
    }
}

fn is_header_footer_candidate(
    element: &ElementBlueprint,
    page_height: u32,
    is_header: bool,
) -> bool {
    if !matches!(
        element.role,
        ElementRole::Title
            | ElementRole::Paragraph
            | ElementRole::ListItem
            | ElementRole::Separator
    ) {
        return false;
    }

    let normalized_text = normalize_repeated_text(&element.text);
    if normalized_text.is_empty() {
        return false;
    }

    let top = element.bounding_box.y;
    let bottom = element
        .bounding_box
        .y
        .saturating_add(element.bounding_box.height);

    if is_header {
        top <= page_height.saturating_mul(14) / 100
    } else {
        bottom >= page_height.saturating_mul(88) / 100
    }
}

fn has_repeated_match(
    candidate: &ElementBlueprint,
    neighbor: &PageBlueprint,
    is_header: bool,
) -> bool {
    let neighbor_height = neighbor.dimensions.height.max(1);
    neighbor.elements.iter().any(|other| {
        is_header_footer_candidate(other, neighbor_height, is_header)
            && same_repeated_pattern(candidate, other, neighbor.dimensions.width.max(1))
    })
}

fn same_repeated_pattern(
    left: &ElementBlueprint,
    right: &ElementBlueprint,
    page_width: u32,
) -> bool {
    if left.role != right.role
        || left.column_index != right.column_index
        || left.total_columns != right.total_columns
    {
        return false;
    }

    let tolerance_x = page_width.saturating_mul(5) / 100;
    let tolerance_w = page_width.saturating_mul(8) / 100;
    let tolerance_y = 36;

    if left.bounding_box.x.abs_diff(right.bounding_box.x) > tolerance_x
        || left.bounding_box.width.abs_diff(right.bounding_box.width) > tolerance_w
        || left.bounding_box.y.abs_diff(right.bounding_box.y) > tolerance_y
    {
        return false;
    }

    let left_text = normalize_repeated_text(&left.text);
    let right_text = normalize_repeated_text(&right.text);
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }

    left_text == right_text || textual_similarity(&left_text, &right_text) >= 0.86
}

fn normalize_repeated_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_space = false;

    for character in text.chars().flat_map(char::to_lowercase) {
        let emitted = if character.is_ascii_digit() {
            Some('#')
        } else if character.is_alphanumeric() {
            Some(character)
        } else if character.is_whitespace()
            || matches!(character, '-' | '–' | '—' | '_' | '/' | ':')
        {
            Some(' ')
        } else {
            None
        };

        match emitted {
            Some(' ') if !previous_space => {
                normalized.push(' ');
                previous_space = true;
            }
            Some(' ') => {}
            Some(value) => {
                normalized.push(value);
                previous_space = false;
            }
            None => {}
        }
    }

    normalized.trim().to_string()
}

fn textual_similarity(left: &str, right: &str) -> f32 {
    if left == right {
        return 1.0;
    }

    let left_len = left.chars().count();
    let right_len = right.chars().count();
    if left_len == 0 || right_len == 0 {
        return 0.0;
    }

    let distance = levenshtein_distance(left, right) as f32;
    let max_len = left_len.max(right_len) as f32;
    1.0 - (distance / max_len)
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0usize; right_chars.len() + 1];

    for (i, left_char) in left_chars.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = if left_char == right_char { 0 } else { 1 };
            current[j + 1] = (current[j] + 1)
                .min(previous[j + 1] + 1)
                .min(previous[j] + substitution_cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}
