use crate::constants::*;
use image::DynamicImage;

pub fn detect_ui_scale(img: &DynamicImage, baseline_margin: f64) -> Option<f64> {
    let (width, height) = (img.width(), img.height());
    let top_h = ((height as f64 * ANCHOR_SCAN_FRACTION) as u32).max(1);
    let rgb = img.to_rgb8();
    let mut best_rightmost_x: Option<u32> = None;

    for y in 0..top_h {
        for x in 0..width {
            let [r, g, b] = rgb.get_pixel(x, y).0;
            if r >= RED_R_MIN && g < RED_G_MAX && b < RED_B_MAX {
                // Density check: count red pixels in a 10x10 box extending to the left.
                // This bridges small gaps that might break strict connectivity.
                let mut count = 0;
                let x_min = x.saturating_sub(9);
                let y_min = y;
                let y_max = (y + 9).min(top_h - 1);

                for bx in x_min..=x {
                    for by in y_min..=y_max {
                        let [br, bg, bb] = rgb.get_pixel(bx, by).0;
                        if br >= RED_R_MIN && bg < RED_G_MAX && bb < RED_B_MAX {
                            count += 1;
                        }
                    }
                }

                if count >= RED_PIXEL_MIN_COUNT {
                    if best_rightmost_x.is_none() || x > best_rightmost_x.unwrap() {
                        best_rightmost_x = Some(x);
                    }
                }
            }
        }
    }

    let rightmost_x = best_rightmost_x?;
    let margin_px = (width - rightmost_x) as f64;
    let ui_scale = margin_px / baseline_margin;

    if ui_scale < UI_SCALE_MIN || ui_scale > UI_SCALE_MAX {
        return None;
    }

    Some(ui_scale)
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

        // Place 15 red pixels in a 3x5 block at x=77..82, y=0..3
        // (x=81 is 19 pixels from the right edge)
        for y in 0..3 {
            for x in 77..82 {
                img.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }

        let dyn_img = DynamicImage::ImageRgb8(img);

        // If baseline_margin is 10, then (100 - 81) / 10 = 1.9 scale
        let scale = detect_ui_scale(&dyn_img, 10.0).unwrap();
        assert!((scale - 1.9).abs() < 1e-6);

        // Test scale out of bounds: (100 - 75) / 10 = 2.5 (> 2.0)
        let mut img_high = RgbImage::new(width, height);
        for y in 0..3 {
            for x in 71..76 {
                img_high.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }
        let dyn_img_high = DynamicImage::ImageRgb8(img_high);
        assert!(detect_ui_scale(&dyn_img_high, 10.0).is_none());

        // Test scale out of bounds: (100 - 96) / 10 = 0.4 (< 0.5)
        let mut img_low = RgbImage::new(width, height);
        for y in 0..3 {
            for x in 92..97 {
                img_low.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }
        let dyn_img_low = DynamicImage::ImageRgb8(img_low);
        assert!(detect_ui_scale(&dyn_img_low, 10.0).is_none());

        // If no red pixels, should return None
        let empty_img = DynamicImage::ImageRgb8(RgbImage::new(100, 100));
        assert!(detect_ui_scale(&empty_img, 10.0).is_none());
    }
}
