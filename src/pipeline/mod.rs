pub mod anchor;
pub mod canvas;
pub mod filter;
pub mod interpolation;
pub mod matcher;
pub mod segment;

use crate::types::{Results, Templates, UiMap};
use image::DynamicImage;

/// Top-level pipeline: detect scale, crop each UI element, extract and match digits.
/// Returns None if the AoE2 UI anchor is not found in the frame.
pub fn process_frame(img: &DynamicImage, ui_map: &UiMap, templates: &Templates) -> Option<Results> {
    // Stage 1
    let ui_scale = anchor::detect_ui_scale(img, ui_map.baseline_margin)?;
    
    let (img_w, img_h) = (img.width(), img.height());
    let mut results = Results::new();
    
    results
        .entry("meta".to_string())
        .or_default()
        .insert("ui_scale".to_string(), format!("{:.4}", ui_scale));

    for (name, coords) in &ui_map.elements {
        let x = (coords.x_px * ui_scale) as u32;
        let y = (coords.y_px * ui_scale) as u32;
        let w = (coords.w_px * ui_scale).max(1.0) as u32;
        let h = (coords.h_px * ui_scale).max(1.0) as u32;

        if x + w > img_w || y + h > img_h {
            eprintln!(
                "Skipping {}: out of bounds ({}+{}>{}, {}+{}>{}",
                name, x, w, img_w, y, h, img_h
            );
            continue;
        }

        let box_dyn = img.crop_imm(x, y, w, h);
        let box_img = box_dyn.to_rgb8();

        // Stage 3: special cases
        let value_str = if name == "idle_vils" {
            if !filter::contains_yellow(&box_img) {
                "0".to_string()
            } else {
                extract_and_match(&box_img, ui_scale, false, false, templates)
            }
        } else if name == "population_total" {
            let overlay = filter::detect_housed_overlay(&box_img);
            let yellow = filter::contains_yellow(&box_img);
            let color = if overlay {
                "overlay"
            } else if yellow {
                "yellow"
            } else {
                "white"
            };

            results
                .entry("population".to_string())
                .or_default()
                .insert("color".to_string(), color.to_string());

            extract_and_match(&box_img, ui_scale, overlay, true, templates)
        } else {
            extract_and_match(&box_img, ui_scale, false, false, templates)
        };

        // Split "wood_total" → category="wood", sub_key="total"
        let (category, sub_key) = match name.splitn(2, '_').collect::<Vec<_>>().as_slice() {
            [cat, sub] => (cat.to_string(), sub.to_string()),
            [cat] => (cat.to_string(), "value".to_string()),
            _ => (name.clone(), "value".to_string()),
        };

        if name == "population_total" {
            if let Some((curr, max)) = value_str.split_once('/') {
                results
                    .entry("population".to_string())
                    .or_default()
                    .insert("total".to_string(), curr.to_string());
                results
                    .entry("population".to_string())
                    .or_default()
                    .insert("housing".to_string(), max.to_string());
            } else {
                results
                    .entry(category)
                    .or_default()
                    .insert(sub_key, value_str);
            }
        } else {
            results
                .entry(category)
                .or_default()
                .insert(sub_key, value_str);
        }
    }

    Some(results)
}

fn extract_and_match(
    box_img: &image::RgbImage,
    ui_scale: f64,
    overlay_mode: bool,
    allow_yellow: bool,
    templates: &Templates,
) -> String {
    // Stage 2: soft-filtered image (source for digit crops)
    let out_img = filter::cleanup_box(box_img, overlay_mode, allow_yellow);

    // Stage 5: segment into individual digit images
    let digit_imgs =
        segment::segment_into_digits(box_img, &out_img, ui_scale, overlay_mode, allow_yellow);

    // Stages 6+7: canvas + match each digit
    digit_imgs
        .iter()
        .map(|d| matcher::match_digit(d, templates))
        .collect()
}
