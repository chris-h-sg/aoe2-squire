use crate::constants::*;
use image::DynamicImage;

pub fn detect_ui_scale(img: &DynamicImage, baseline_margin: f64) -> Option<f64> {
    let (width, height) = (img.width(), img.height());

    let y_min_scan = (height as f64 * ANCHOR_MIN_Y) as u32;
    let y_max_scan = ((height as f64 * ANCHOR_MAX_Y) as u32).min(height - 1);
    let x_min_scan = (width as f64 * ANCHOR_MIN_X) as u32;
    let x_max_scan = ((width as f64 * ANCHOR_MAX_X) as u32).min(width - 1);

    let rgb = img.to_rgb8();
    let mut best_rightmost_x: Option<u32> = None;

    for y in y_min_scan..=y_max_scan {
        for x in x_min_scan..=x_max_scan {
            let [r, g, b] = rgb.get_pixel(x, y).0;
            let (r_i, g_i, b_i) = (r as i16, g as i16, b as i16);

            // Red must be at least RED_DIFF_THRESHOLD higher than both Green and Blue
            if r_i - g_i >= RED_DIFF_THRESHOLD && r_i - b_i >= RED_DIFF_THRESHOLD {
                // Density check: count red pixels in a 10x10 box extending to the left.
                let mut count = 0;
                let x_min = x.saturating_sub(9).max(x_min_scan);
                let y_min = y;
                let y_max = (y + 9).min(y_max_scan);

                for bx in x_min..=x {
                    for by in y_min..=y_max {
                        let [br, bg, bb] = rgb.get_pixel(bx, by).0;
                        let (br_i, bg_i, bb_i) = (br as i16, bg as i16, bb as i16);
                        if br_i - bg_i >= RED_DIFF_THRESHOLD && br_i - bb_i >= RED_DIFF_THRESHOLD {
                            count += 1;
                        }
                    }
                }

                if count >= RED_PIXEL_MIN_COUNT
                    && (best_rightmost_x.is_none() || x > best_rightmost_x.unwrap())
                {
                    best_rightmost_x = Some(x);
                }
            }
        }
    }

    let rightmost_x = best_rightmost_x?;
    let margin_px = (width - rightmost_x) as f64;
    let ui_scale = margin_px / baseline_margin;

    if !(UI_SCALE_MIN..=UI_SCALE_MAX).contains(&ui_scale) {
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

        // Place 15 red pixels in a 3x5 block at x=77..82, y=1..4
        // (x=81 is 19 pixels from the right edge)
        // y=1 is within the scan area (1.5% to 3.5% of 100 = 1..3)
        for y in 1..4 {
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
        for y in 1..4 {
            for x in 71..76 {
                img_high.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }
        let dyn_img_high = DynamicImage::ImageRgb8(img_high);
        assert!(detect_ui_scale(&dyn_img_high, 10.0).is_none());

        // Test scale out of bounds: (100 - 96) / 10 = 0.4 (< 0.5)
        // Note: x=96 is also outside the scan area (max 95), so this should return None.
        let mut img_low = RgbImage::new(width, height);
        for y in 1..4 {
            for x in 96..100 {
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
