use image::DynamicImage;
use crate::constants::*;

/// Stage 1: detect UI scale from the rightmost red pixel in the top 5% of the frame.
/// Returns None if fewer than RED_PIXEL_MIN_COUNT red pixels are found (game not visible).
pub fn detect_ui_scale(img: &DynamicImage) -> Option<f64> {
    let (width, height) = (img.width(), img.height());
    let top_h = ((height as f64 * 0.05) as u32).max(1);
    let rgb = img.to_rgb8();

    let mut rightmost_x: Option<u32> = None;
    let mut red_count = 0usize;

    for y in 0..top_h {
        for x in 0..width {
            let [r, g, b] = rgb.get_pixel(x, y).0;
            if r >= RED_R_MIN && g < RED_G_MAX && b < RED_B_MAX {
                red_count += 1;
                rightmost_x = Some(rightmost_x.map_or(x, |rx| rx.max(x)));
            }
        }
    }

    if red_count < RED_PIXEL_MIN_COUNT {
        return None;
    }

    let margin_px = (width - rightmost_x?) as f64;
    Some(margin_px / BASELINE_MARGIN)
}
