use crate::constants::*;
use image::DynamicImage;

/// Stage 1: detect UI scale from the rightmost red pixel in the top 5% of the frame.
/// Returns None if fewer than RED_PIXEL_MIN_COUNT red pixels are found (game not visible).
pub fn detect_ui_scale(img: &DynamicImage, baseline_margin: f64) -> Option<f64> {
    let (width, height) = (img.width(), img.height());
    let top_h = ((height as f64 * ANCHOR_SCAN_FRACTION) as u32).max(1);
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
    Some(margin_px / baseline_margin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_detect_ui_scale() {
        let width = 100;
        let height = 100;
        let mut img = RgbImage::new(width, height);

        // Place some red pixels at x=80
        // (x=80 is 20 pixels from the right edge)
        for y in 0..5 {
            img.put_pixel(80, y, Rgb([255, 0, 0]));
        }

        let dyn_img = DynamicImage::ImageRgb8(img);

        // If baseline_margin is 10, then 20 / 10 = 2.0 scale
        let scale = detect_ui_scale(&dyn_img, 10.0).unwrap();
        assert!((scale - 2.0).abs() < 1e-6);

        // If no red pixels, should return None
        let empty_img = DynamicImage::ImageRgb8(RgbImage::new(100, 100));
        assert!(detect_ui_scale(&empty_img, 10.0).is_none());
    }
}
