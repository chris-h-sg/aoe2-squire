use image::GrayImage;
use crate::constants::*;

// TODO: implement canvas preparation (stage 6).
// Steps:
// 1. Scale height to WORKING_HEIGHT (preserve aspect ratio).
//    Use FilterType::CatmullRom if upscaling, FilterType::Triangle if downscaling.
// 2. Find bounding rect of non-zero pixels; crop to it.
// 3. Center crop in CANVAS_SIZE × CANVAS_SIZE canvas (zero-padded).
// 4. Gaussian blur sigma=BLUR_SIGMA (imageproc::filter::gaussian_blur_f32 on Luma<f32> buffer).
// 5. Convert to f32 [0.0, 1.0]; return as flat Vec<f32> of length CANVAS_SIZE².

/// Converts a digit crop into the standard 64×64 float canvas used for SSD matching.
/// Stub: returns a zeroed canvas until the full implementation is wired in.
pub fn prepare_canvas(_img: &GrayImage) -> Vec<f32> {
    vec![0.0f32; (CANVAS_SIZE * CANVAS_SIZE) as usize]
}
