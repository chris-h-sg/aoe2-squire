use image::{GrayImage, ImageBuffer, Luma, RgbImage};
use imageproc::region_labelling::{connected_components, Connectivity};
use crate::constants::*;
use super::filter::apply_base_filter;

struct CompStats {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    area: u32,
    max_brightness: u8,
}

struct BoundingBox {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn compute_stats(
    labeled: &ImageBuffer<Luma<u32>, Vec<u32>>,
    seg: &GrayImage,
    max_label: u32,
) -> Vec<(usize, CompStats)> {
    let n = max_label as usize + 1;
    let mut left  = vec![u32::MAX; n];
    let mut top   = vec![u32::MAX; n];
    let mut right = vec![0u32; n];
    let mut bot   = vec![0u32; n];
    let mut area  = vec![0u32; n];
    let mut max_b = vec![0u8; n];

    for (x, y, p) in labeled.enumerate_pixels() {
        let label = p.0[0] as usize;
        if label == 0 {
            continue;
        }
        let br = seg.get_pixel(x, y).0[0];
        if x < left[label]  { left[label]  = x; }
        if y < top[label]   { top[label]   = y; }
        if x > right[label] { right[label] = x; }
        if y > bot[label]   { bot[label]   = y; }
        area[label] += 1;
        if br > max_b[label] { max_b[label] = br; }
    }

    (1..n)
        .filter_map(|i| {
            if area[i] == 0 {
                return None;
            }
            Some((i, CompStats {
                left:           left[i],
                top:            top[i],
                width:          right[i] - left[i] + 1,
                height:         bot[i]   - top[i]  + 1,
                area:           area[i],
                max_brightness: max_b[i],
            }))
        })
        .collect()
}

/// Binarize a grayscale image: non-zero → 255, zero → 0.
/// imageproc::connected_components groups by identical pixel value, so we must
/// collapse all foreground intensities to a single value before labeling.
fn binarize(seg: &GrayImage) -> GrayImage {
    ImageBuffer::from_fn(seg.width(), seg.height(), |x, y| {
        Luma([if seg.get_pixel(x, y).0[0] > 0 { 255u8 } else { 0u8 }])
    })
}

/// Recursive connected-components segmentation.
/// Returns bounding boxes in the coordinate space of the *original* box_img
/// (offsets accumulate via offset_x / offset_y).
fn get_components_recursive(
    roi: &RgbImage,
    grey_tol: i16,
    brightness_thresh: u8,
    use_eight: bool,
    offset_x: u32,
    offset_y: u32,
    min_area: u32,
    overlay_mode: bool,
    allow_yellow: bool,
) -> Vec<BoundingBox> {
    let seg = apply_base_filter(roi, grey_tol, brightness_thresh, overlay_mode, allow_yellow);
    let binary = binarize(&seg);
    let conn = if use_eight { Connectivity::Eight } else { Connectivity::Four };
    let labeled = connected_components(&binary, conn, Luma([0u8]));

    let max_label = labeled.pixels().map(|p| p.0[0]).max().unwrap_or(0);
    if max_label == 0 {
        return vec![];
    }

    let all_stats = compute_stats(&labeled, &seg, max_label);
    let valid: Vec<(usize, CompStats)> = all_stats
        .into_iter()
        .filter(|(_, s)| s.area >= min_area && s.max_brightness >= SEG_REQUIRED_BRIGHTNESS)
        .collect();

    if valid.is_empty() {
        return vec![];
    }

    let (roi_w, roi_h) = roi.dimensions();

    // Case A: single wide blob that fills the entire ROI → try harder to split.
    if valid.len() == 1 {
        let (_, ref s) = valid[0];
        if s.width == roi_w && s.height == roi_h && s.width + 2 > s.height {
            if brightness_thresh < 160 {
                return get_components_recursive(
                    roi, grey_tol, brightness_thresh + 10, use_eight,
                    offset_x, offset_y, min_area, overlay_mode, allow_yellow,
                );
            }
            if grey_tol > 2 {
                return get_components_recursive(
                    roi, grey_tol - 2, brightness_thresh, use_eight,
                    offset_x, offset_y, min_area, overlay_mode, allow_yellow,
                );
            }
            if use_eight {
                return get_components_recursive(
                    roi, grey_tol, brightness_thresh, false,
                    offset_x, offset_y, min_area, overlay_mode, allow_yellow,
                );
            }
            // All strategies exhausted: return as a single digit.
            return vec![BoundingBox { x: offset_x, y: offset_y, w: s.width, h: s.height }];
        }
    }

    // Case B: standard processing — recurse on wide components, accept narrow ones.
    let mut results = vec![];
    for (label_idx, s) in &valid {
        if s.width + 2 > s.height {
            // Build an isolated sub-image: only pixels belonging to this component.
            let mut sub = RgbImage::new(s.width, s.height);
            for py in 0..s.height {
                for px in 0..s.width {
                    let gx = s.left + px;
                    let gy = s.top + py;
                    if gx < roi_w && gy < roi_h
                        && labeled.get_pixel(gx, gy).0[0] as usize == *label_idx
                    {
                        sub.put_pixel(px, py, *roi.get_pixel(gx, gy));
                    }
                }
            }
            let sub_boxes = get_components_recursive(
                &sub, grey_tol, brightness_thresh, use_eight,
                offset_x + s.left, offset_y + s.top,
                min_area, overlay_mode, allow_yellow,
            );
            results.extend(sub_boxes);
        } else {
            results.push(BoundingBox {
                x: offset_x + s.left,
                y: offset_y + s.top,
                w: s.width,
                h: s.height,
            });
        }
    }
    results
}

/// Stage 5: segment `box_img` into individual digit images cropped from `out_img`.
/// Returns digits sorted left-to-right.
pub fn segment_into_digits(
    box_img: &RgbImage,
    out_img: &GrayImage,
    ui_scale: f64,
    overlay_mode: bool,
    allow_yellow: bool,
) -> Vec<GrayImage> {
    let min_area = ((BASELINE_MIN_AREA * ui_scale * ui_scale) as u32).max(1);

    let mut digit_boxes = get_components_recursive(
        box_img,
        SEG_GREY_TOLERANCE,
        SEG_BRIGHTNESS_THRESHOLD,
        true,
        0, 0,
        min_area,
        overlay_mode,
        allow_yellow,
    );

    digit_boxes.sort_by_key(|d| d.x);

    let (ow, oh) = out_img.dimensions();
    digit_boxes
        .into_iter()
        .filter_map(|db| {
            let x = db.x.min(ow.saturating_sub(1));
            let y = db.y.min(oh.saturating_sub(1));
            let w = db.w.min(ow.saturating_sub(x));
            let h = db.h.min(oh.saturating_sub(y));
            if w == 0 || h == 0 {
                return None;
            }
            Some(image::imageops::crop_imm(out_img, x, y, w, h).to_image())
        })
        .collect()
}
