pub mod anchor;
pub mod canvas;
pub mod filter;
pub mod interpolation;
pub mod matcher;
pub mod segment;

use crate::constants::{ANNE_HK_IDLE_W_ADJUST_FACTOR, ANNE_HK_Y_ADJUST_FACTOR};
use crate::types::{Results, Templates, UiElement, UiMap};
use image::{DynamicImage, RgbImage};

struct ExtractorPipeline<'a> {
    ui_map: &'a UiMap,
    templates: &'a Templates,
    ui_scale: f64,
    anne_hk_active: bool,
    ref_baseline: Option<(u32, u32)>,
    vil_ref_baseline: Option<(u32, u32)>,
}

impl<'a> ExtractorPipeline<'a> {
    fn new(ui_map: &'a UiMap, templates: &'a Templates) -> Self {
        Self {
            ui_map,
            templates,
            ui_scale: 1.0,
            anne_hk_active: false,
            ref_baseline: None,
            vil_ref_baseline: None,
        }
    }

    fn get_raw_coords(&self, coords: &UiElement) -> (u32, u32, u32, u32) {
        (
            (coords.x_px * self.ui_scale) as u32,
            (coords.y_px * self.ui_scale) as u32,
            (coords.w_px * self.ui_scale).max(1.0) as u32,
            (coords.h_px * self.ui_scale).max(1.0) as u32,
        )
    }

    fn calibrate(&mut self, img: &DynamicImage) -> bool {
        if let Some(scale) = anchor::detect_ui_scale(img, self.ui_map.baseline_margin) {
            self.ui_scale = scale;
        } else {
            return false;
        }

        let rgb = img.to_rgb8();
        let (img_w, img_h) = (rgb.width(), rgb.height());

        // Mod detection
        if let Some(wood_vils) = self.ui_map.elements.get("wood_vils") {
            let (x, y, w, h) = self.get_raw_coords(wood_vils);
            if x + w <= img_w && y + h <= img_h && filter::detect_anne_hk_mod(&rgb, x, y, w, h) {
                self.anne_hk_active = true;
            }
        }

        if let Some(wood_total) = self.ui_map.elements.get("wood_total") {
            self.ref_baseline = self.detect_baseline(&rgb, "wood_total", wood_total);
        }
        if let Some(wood_vils) = self.ui_map.elements.get("wood_vils") {
            self.vil_ref_baseline = self.detect_baseline(&rgb, "wood_vils", wood_vils);
        }
        true
    }

    fn detect_baseline(
        &self,
        img: &RgbImage,
        name: &str,
        coords: &UiElement,
    ) -> Option<(u32, u32)> {
        let (x, mut y, w, mut h) = self.get_raw_coords(coords);
        let (img_w, img_h) = (img.width(), img.height());

        if self.anne_hk_active && name.ends_with("_vils") {
            let y_adj = (h as f64 * ANNE_HK_Y_ADJUST_FACTOR) as u32;
            y = y.saturating_sub(y_adj);
            h += y_adj;
        }

        if x + w > img_w || y + h > img_h {
            return None;
        }

        let box_img = image::imageops::crop_imm(img, x, y, w, h).to_image();
        let ignore_color = self.anne_hk_active && name.ends_with("_vils");

        segment::detect_baseline(&box_img, self.ui_scale, ignore_color)
    }

    fn get_element_coords(&self, name: &str, coords: &UiElement) -> (u32, u32, u32, u32, bool) {
        let (x, mut y, mut w, mut h) = self.get_raw_coords(coords);
        let mut clipped = false;

        if self.anne_hk_active && name.ends_with("_vils") {
            let y_adj = (h as f64 * ANNE_HK_Y_ADJUST_FACTOR) as u32;
            y = y.saturating_sub(y_adj);
            h += y_adj;
            if name == "idle_vils" {
                w += (w as f64 * ANNE_HK_IDLE_W_ADJUST_FACTOR) as u32;
            }
        }

        let target_ref = if name == "idle_vils" {
            None
        } else if name.ends_with("_vils") {
            self.vil_ref_baseline
        } else {
            self.ref_baseline
        };

        if let Some((ref_h, ref_y)) = target_ref {
            y += ref_y;
            h = ref_h;
            clipped = true;
        }

        (x, y, w, h, clipped)
    }

    fn process_box(
        &self,
        img: &RgbImage,
        name: &str,
        coords: &UiElement,
    ) -> Option<(RgbImage, Vec<image::GrayImage>, bool, Option<[u8; 3]>)> {
        let (x, y, w, h, clipped) = self.get_element_coords(name, coords);
        if x + w > img.width() || y + h > img.height() {
            return None;
        }

        let box_img = image::imageops::crop_imm(img, x, y, w, h).to_image();
        let ignore_color = self.anne_hk_active && name.ends_with("_vils");
        let allow_yellow = name == "population_total";

        // Sample background colour once from the root box before any segmentation.
        // Passed as Option<[u8; 3]> so recursive sub-image calls never re-sample.
        let overlay_bg = if name == "population_total" && filter::detect_housed_overlay(&box_img) {
            let (w_box, h_box) = box_img.dimensions();
            if w_box > 2 && h_box > 2 {
                Some(box_img.get_pixel(2, 2).0)
            } else {
                None
            }
        } else {
            None
        };

        let out_img = filter::cleanup_box(&box_img, overlay_bg, allow_yellow, ignore_color);

        if name == "idle_vils" {
            let active = if self.anne_hk_active {
                filter::contains_red(&box_img)
            } else {
                filter::contains_yellow(&box_img)
            };
            if !active {
                return Some((box_img, vec![], clipped, None));
            }
        }

        let digits = segment::segment_into_digits(
            &box_img,
            &out_img,
            self.ui_scale,
            overlay_bg,
            allow_yellow,
            ignore_color,
        );
        Some((box_img, digits, clipped, overlay_bg))
    }

    fn run(&mut self, img: &DynamicImage) -> Option<Results> {
        if !self.calibrate(img) {
            return None;
        }

        let rgb = img.to_rgb8();
        let mut results = Results::new();
        let mut pop_color = "white";

        results
            .entry("meta".to_string())
            .or_default()
            .insert("ui_scale".to_string(), format!("{:.4}", self.ui_scale));

        for (name, coords) in &self.ui_map.elements {
            if let Some((box_img, digits, _clipped, overlay_bg)) = self.process_box(&rgb, name, coords) {
                let mut match_results: Vec<(char, f32)> = digits
                    .iter()
                    .enumerate()
                    .map(|(idx, d)| {
                        let allow_slash = name == "population_total"
                            && digits.len() > 2
                            && idx > 0
                            && idx < digits.len() - 1;
                        matcher::match_digit(d, self.templates, allow_slash)
                    })
                    .collect();

                if name == "population_total"
                    && digits.len() > 2
                    && !match_results.iter().any(|m| m.0 == '/')
                {
                    let mut best_idx = None;
                    let mut min_diff = f32::INFINITY;
                    for (i, d) in digits.iter().enumerate() {
                        if i > 0 && i < digits.len() - 1 {
                            let ssd_slash = matcher::calculate_ssd_for_char(d, '/', self.templates);
                            let diff = ssd_slash - match_results[i].1;
                            if diff < min_diff {
                                min_diff = diff;
                                best_idx = Some(i);
                            }
                        }
                    }
                    if let Some(idx) = best_idx {
                        match_results[idx].0 = '/';
                    }
                }

                let mut value_str: String = match_results.iter().map(|m| m.0).collect();

                if name == "idle_vils" && digits.is_empty() {
                    value_str = "0".to_string();
                }

                if name == "population_total" {
                    if overlay_bg.is_some() {
                        pop_color = "overlay";
                    } else if filter::contains_yellow(&box_img) {
                        pop_color = "yellow";
                    }
                }

                let (category, sub_key) = match name.splitn(2, '_').collect::<Vec<_>>().as_slice() {
                    [cat, sub] => (cat.to_string(), sub.to_string()),
                    [cat] => (cat.to_string(), "value".to_string()),
                    _ => (name.clone(), "value".to_string()),
                };

                if name == "population_total" {
                    if let Some((curr, max)) = value_str.split_once('/') {
                        results
                            .entry(category.clone())
                            .or_default()
                            .insert("total".to_string(), curr.to_string());
                        results
                            .entry(category)
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
        }

        results
            .entry("population".to_string())
            .or_default()
            .insert("color".to_string(), pop_color.to_string());

        Some(results)
    }
}

pub fn process_frame(img: &DynamicImage, ui_map: &UiMap, templates: &Templates) -> Option<Results> {
    let mut pipeline = ExtractorPipeline::new(ui_map, templates);
    pipeline.run(img)
}
