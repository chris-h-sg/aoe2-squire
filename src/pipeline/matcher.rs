use super::canvas;
use crate::constants::*;
use crate::types::Templates;
use image::GrayImage;
use include_dir::{include_dir, Dir};
use std::path::Path;

static TEMPLATES_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/research/templates/enormous_numbers");

/// Loads and pre-processes all digit templates from the embedded binary.
pub fn load_embedded_templates() -> Templates {
    let mut templates = Templates::new();

    for file in TEMPLATES_DIR.files() {
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let ch: char = if stem == "slash" {
            '/'
        } else if stem.len() == 1 {
            stem.chars().next().unwrap()
        } else {
            continue;
        };

        let img = match image::load_from_memory(file.contents()) {
            Ok(i) => i.to_luma8(),
            Err(e) => {
                eprintln!("Warning: failed to load embedded template {:?}: {}", path, e);
                continue;
            }
        };

        templates.insert(ch, canvas::prepare_canvas(&img));
    }

    templates
}

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
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
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

/// Stage 7: match a single digit image against all templates via wiggle SSD.
/// Returns the best-matching character, or '?' if no templates are loaded.
pub fn match_digit(digit: &GrayImage, templates: &Templates) -> char {
    if templates.is_empty() {
        return '?';
    }

    let input_canvas = canvas::prepare_canvas(digit);
    let size = CANVAS_SIZE as usize;

    // Pre-compute 9 shifted versions of the input canvas.
    // shifted[y][x] = input[y+dy][x+dx] (out-of-bounds filled with 0).
    // This is equivalent to Python's warpAffine with M=[[1,0,-dx],[0,1,-dy]],
    // which maps dst(x,y) ← src(x-dx, y-dy) → same result set over all ±1 offsets.
    let shifted_inputs: Vec<Vec<f32>> = WIGGLE_OFFSETS
        .iter()
        .flat_map(|&dy| WIGGLE_OFFSETS.iter().map(move |&dx| (dy, dx)))
        .map(|(dy, dx)| {
            let mut shifted = vec![0.0f32; size * size];
            for y in 0..size {
                for x in 0..size {
                    let sx = x as i32 + dx;
                    let sy = y as i32 + dy;
                    if sx >= 0 && sx < size as i32 && sy >= 0 && sy < size as i32 {
                        shifted[y * size + x] = input_canvas[sy as usize * size + sx as usize];
                    }
                }
            }
            shifted
        })
        .collect();

    // Compute minimum SSD across all 9 shifts for each template.
    let mut matches: Vec<(f32, char)> = templates
        .iter()
        .map(|(&ch, template)| {
            let best_ssd = shifted_inputs
                .iter()
                .map(|shifted| {
                    shifted
                        .iter()
                        .zip(template.iter())
                        .map(|(&a, &b)| {
                            let d = a - b;
                            d * d
                        })
                        .sum::<f32>()
                })
                .fold(f32::INFINITY, f32::min);
            (best_ssd, ch)
        })
        .collect();

    matches.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Symmetry tie-breaker: if winner is '0' vs '3'/'6'/'9' (or vice versa)
    // and margin < 20% of winner SSD, use horizontal symmetry to decide.
    if matches.len() > 1 {
        let (best_ssd, best_char) = matches[0];
        let (second_ssd, second_char) = matches[1];

        let asym = ['3', '6', '9'];
        let is_conflict = (best_char == '0' && asym.contains(&second_char))
            || (asym.contains(&best_char) && second_char == '0');

        if is_conflict && second_ssd - best_ssd < best_ssd * 0.20 {
            let sym_score = h_symmetry_score(&input_canvas, size);
            // '0' is symmetric (score < 40); '3'/'6'/'9' are asymmetric (score > 60).
            if (best_char == '0' && sym_score > 60.0)
                || (asym.contains(&best_char) && sym_score < 40.0)
            {
                matches.swap(0, 1);
            }
        }
    }

    matches[0].1
}

/// sum((canvas - hflip(canvas))²) — lower means more horizontally symmetric.
fn h_symmetry_score(canvas: &[f32], size: usize) -> f32 {
    let mut sum = 0.0f32;
    for y in 0..size {
        for x in 0..size {
            let a = canvas[y * size + x];
            let b = canvas[y * size + (size - 1 - x)];
            let d = a - b;
            sum += d * d;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_h_symmetry_score() {
        let size = 10;
        let mut canvas = vec![0.0f32; size * size];

        // Perfectly symmetric pattern: vertical line in middle
        for y in 0..size {
            canvas[y * size + 4] = 1.0;
            canvas[y * size + 5] = 1.0;
        }
        let score_sym = h_symmetry_score(&canvas, size);
        assert!(score_sym < 1.0);

        // Asymmetric pattern: line on one side
        let mut canvas_asym = vec![0.0f32; size * size];
        for y in 0..size {
            canvas_asym[y * size + 2] = 1.0;
        }
        let score_asym = h_symmetry_score(&canvas_asym, size);
        assert!(score_asym > 5.0);
    }
}
