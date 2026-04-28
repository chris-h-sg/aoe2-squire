use image::{GrayImage, RgbImage};

// TODO: implement connected-components digit segmentation (stage 5).
// - Run connected_components on apply_base_filter(SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD)
// - Discard components with area < BASELINE_MIN_AREA * ui_scale²
// - Discard components with no pixel >= SEG_REQUIRED_BRIGHTNESS
// - Recurse on wide blobs (w+2 > h): tighten brightness_threshold (+10 to 160),
//   then grey_tol (-2 to 2), then switch to 4-connectivity
// - Sort survivors left-to-right, crop from out_img (not the seg image)

/// Returns ordered list of digit crop images taken from `out_img`.
/// Stub: returns the entire `out_img` as a single digit until segmentation is implemented.
pub fn segment_into_digits(
    _box_img: &RgbImage,
    out_img: &GrayImage,
    _ui_scale: f64,
    _overlay_mode: bool,
    _allow_yellow: bool,
) -> Vec<GrayImage> {
    vec![out_img.clone()]
}
