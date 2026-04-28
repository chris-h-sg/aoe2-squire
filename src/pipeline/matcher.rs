use std::path::Path;
use image::GrayImage;
use crate::types::Templates;
use super::canvas;

// TODO: implement wiggle SSD matching (stage 7).
// Algorithm:
// 1. prepare_canvas(digit_img) → input canvas (64×64 f32).
// 2. Pre-shift input 9 times (dx, dy in WIGGLE_OFFSETS × WIGGLE_OFFSETS) — simple array roll,
//    fill vacated edge with 0 (equivalent to warpAffine translate).
// 3. For each template: min SSD across the 9 shifts.
// 4. Sort by SSD ascending.
// 5. Tie-breaker: if winner/runner-up are {0} vs {3,6,9} and margin < 20% of winner SSD:
//    compute horizontal symmetry score = sum((canvas - hflip(canvas))²).
//    0 is symmetric (score < 40); 3/6/9 are asymmetric (score > 60). Swap if contradicted.

/// Loads and pre-processes all digit templates from `dir`.
/// Each PNG is named "0.png" … "9.png" and "slash.png" (mapped to '/').
pub fn load_templates(dir: &Path) -> Templates {
    let mut templates = Templates::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Warning: could not read templates dir {:?}: {}", dir, e);
            return templates;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let ch: char = if stem == "slash" {
            '/'
        } else if stem.len() == 1 {
            stem.chars().next().unwrap()
        } else {
            continue;
        };

        let img = match image::open(&path) {
            Ok(i) => i.to_luma8(),
            Err(e) => {
                eprintln!("Warning: failed to load template {:?}: {}", path, e);
                continue;
            }
        };

        templates.insert(ch, canvas::prepare_canvas(&img));
    }

    templates
}

/// Stage 7: match a single digit image against all templates, return the best character.
/// Stub: always returns '?' until the SSD matcher is implemented.
pub fn match_digit(_digit: &GrayImage, _templates: &Templates) -> char {
    '?'
}
