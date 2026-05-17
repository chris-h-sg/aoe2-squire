use crate::constants::*;
use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma};
use imageproc::filter::gaussian_blur_f32;

/// Stage 6: scale → crop-to-content → center in 64×64 → Gaussian blur → f32 [0,1] flat vec.
pub fn prepare_canvas(img: &GrayImage) -> Vec<f32> {
    let empty = || vec![0.0f32; (CANVAS_SIZE * CANVAS_SIZE) as usize];

    let (w_orig, h_orig) = img.dimensions();
    if w_orig == 0 || h_orig == 0 {
        return empty();
    }

    // 1. Scale height to WORKING_HEIGHT, preserve aspect ratio.
    let scale = WORKING_HEIGHT as f64 / h_orig as f64;
    let new_w = ((w_orig as f64 * scale) as u32).max(1);
    let filter = if scale > 1.0 {
        FilterType::Lanczos3
    } else {
        FilterType::Triangle
    };
    let upscaled = image::imageops::resize(img, new_w, WORKING_HEIGHT, filter);

    // 2. Find bounding box of pixels with value > 1 (mirrors cv2.threshold at 1).
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for (x, y, p) in upscaled.enumerate_pixels() {
        if p.0[0] > 1 {
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            found = true;
        }
    }
    if !found {
        return empty();
    }

    let bb_w = max_x - min_x + 1;
    let bb_h = max_y - min_y + 1;
    let mut crop: GrayImage =
        image::imageops::crop_imm(&upscaled, min_x, min_y, bb_w, bb_h).to_image();

    // Safety: resize down if the crop is somehow larger than the canvas.
    let mut cw = crop.width();
    let mut ch = crop.height();
    if cw > CANVAS_SIZE || ch > CANVAS_SIZE {
        let scale_fit = (CANVAS_SIZE as f64 / ch as f64).min(CANVAS_SIZE as f64 / cw as f64);
        let fw = ((cw as f64 * scale_fit) as u32).clamp(1, CANVAS_SIZE);
        let fh = ((ch as f64 * scale_fit) as u32).clamp(1, CANVAS_SIZE);
        crop = image::imageops::resize(&crop, fw, fh, filter);
        cw = crop.width();
        ch = crop.height();
    }

    // 3. Center the cropped digit inside a CANVAS_SIZE x CANVAS_SIZE canvas.
    let off_x = ((CANVAS_SIZE - cw) / 2) as i64;
    let off_y = ((CANVAS_SIZE - ch) / 2) as i64;
    let mut canvas = GrayImage::new(CANVAS_SIZE, CANVAS_SIZE);
    image::imageops::overlay(&mut canvas, &crop, off_x, off_y);

    // 4. Convert to f32 [0,1], apply Gaussian blur, return flat vec.
    let canvas_f: ImageBuffer<Luma<f32>, Vec<f32>> =
        ImageBuffer::from_fn(CANVAS_SIZE, CANVAS_SIZE, |x, y| {
            Luma([canvas.get_pixel(x, y).0[0] as f32 / 255.0])
        });
    gaussian_blur_f32(&canvas_f, BLUR_SIGMA).into_raw()
}
