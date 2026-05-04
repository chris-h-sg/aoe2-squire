use crate::constants::*;
use image::{GrayImage, Luma, RgbImage};

/// Stage 3 helper: true if mean(R)>150, mean(G)>150, mean(B)<100 (housed overlay detected).
/// Python uses BGR ordering; we use RGB — channel semantics are re-mapped accordingly.
pub(super) fn detect_housed_overlay(img: &RgbImage) -> bool {
    let npx = (img.width() * img.height()) as u64;
    if npx == 0 {
        return false;
    }
    let (sum_r, sum_g, sum_b) = img.pixels().fold((0u64, 0u64, 0u64), |(sr, sg, sb), p| {
        (sr + p.0[0] as u64, sg + p.0[1] as u64, sb + p.0[2] as u64)
    });
    let mean_r = sum_r as f64 / npx as f64;
    let mean_g = sum_g as f64 / npx as f64;
    let mean_b = sum_b as f64 / npx as f64;
    mean_r > 150.0 && mean_g > 150.0 && mean_b < 100.0
}

/// Stage 3 helper: true if any pixel satisfies yellow criteria (idle vils icon is active).
/// Criteria: R>100, G>100, B < min(R,G)-30, |R-G| < 50.
pub(super) fn contains_yellow(img: &RgbImage) -> bool {
    img.pixels().any(|p| {
        let [r, g, b] = p.0;
        let r16 = r as i16;
        let g16 = g as i16;
        let b16 = b as i16;
        r > YELLOW_MIN_BRIGHTNESS
            && g > YELLOW_MIN_BRIGHTNESS
            && b16 < r16.min(g16) - YELLOW_BLUE_MARGIN
            && (r16 - g16).abs() < YELLOW_RG_SIMILARITY
    })
}

/// Core pixel filter — two modes:
///
/// Standard (overlay_mode=false): keep grey/white pixels (max-min ≤ grey_tol), output max channel.
/// If allow_yellow, also pass through yellow pixels (R>150, G>150, |R-G|<50, B<max-15).
///
/// Overlay (overlay_mode=true): subtract background colour at (2,2), convert to luminance,
/// NORM_MINMAX, threshold. Used for population_total when housed.
pub fn apply_base_filter(
    img: &RgbImage,
    grey_tol: i16,
    brightness_thresh: u8,
    overlay_mode: bool,
    allow_yellow: bool,
) -> GrayImage {
    let (w, h) = img.dimensions();

    if overlay_mode {
        let bg = img.get_pixel(2, 2).0;
        let (bg_r, bg_g, bg_b) = (bg[0] as i16, bg[1] as i16, bg[2] as i16);

        // Subtract background per-pixel, convert to luminance
        let lumas: Vec<u8> = img
            .pixels()
            .map(|p| {
                let [r, g, b] = p.0;
                let sr = (r as i16 - bg_r).max(0) as f32;
                let sg = (g as i16 - bg_g).max(0) as f32;
                let sb = (b as i16 - bg_b).max(0) as f32;
                (0.299 * sr + 0.587 * sg + 0.114 * sb) as u8
            })
            .collect();

        // NORM_MINMAX: scale [min, max] → [0, 255]
        let lo = *lumas.iter().min().unwrap_or(&0) as f32;
        let hi = *lumas.iter().max().unwrap_or(&0) as f32;
        let range = hi - lo;

        let mut out = GrayImage::new(w, h);
        for (i, (x, y, _)) in img.enumerate_pixels().enumerate() {
            let norm = if range > 0.0 {
                ((lumas[i] as f32 - lo) / range * 255.0) as u8
            } else {
                0
            };
            out.put_pixel(
                x,
                y,
                Luma([if norm >= brightness_thresh { norm } else { 0 }]),
            );
        }
        return out;
    }

    // Standard mode
    let mut out = GrayImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels() {
        let [r, g, b] = p.0;
        let r16 = r as i16;
        let g16 = g as i16;
        let b16 = b as i16;
        let max_ch = r16.max(g16).max(b16);
        let min_ch = r16.min(g16).min(b16);

        let mut keep = (max_ch - min_ch) <= grey_tol;

        if allow_yellow && !keep {
            let rg_diff = (r16 - g16).abs();
            if r > 150 && g > 150 && rg_diff < 50 && b16 < max_ch - 15 {
                keep = true;
            }
        }

        if keep && max_ch as u8 >= brightness_thresh {
            out.put_pixel(x, y, Luma([max_ch as u8]));
        }
    }
    out
}

/// Stage 2 wrapper: produces the soft-filtered output image used by the matcher.
pub fn cleanup_box(img: &RgbImage, overlay_mode: bool, allow_yellow: bool) -> GrayImage {
    let thresh = if overlay_mode {
        OUT_OVERLAY_BRIGHTNESS_THRESHOLD
    } else {
        OUT_BRIGHTNESS_THRESHOLD
    };
    apply_base_filter(img, OUT_GREY_TOLERANCE, thresh, overlay_mode, allow_yellow)
}
