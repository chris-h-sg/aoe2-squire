use super::filter::apply_base_filter;
use crate::constants::*;
use image::{GrayImage, ImageBuffer, Luma, RgbImage};
use imageproc::region_labelling::{connected_components, Connectivity};

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
    let mut left = vec![u32::MAX; n];
    let mut top = vec![u32::MAX; n];
    let mut right = vec![0u32; n];
    let mut bot = vec![0u32; n];
    let mut area = vec![0u32; n];
    let mut max_b = vec![0u8; n];

    for (x, y, p) in labeled.enumerate_pixels() {
        let label = p.0[0] as usize;
        if label == 0 {
            continue;
        }
        let br = seg.get_pixel(x, y).0[0];
        if x < left[label] {
            left[label] = x;
        }
        if y < top[label] {
            top[label] = y;
        }
        if x > right[label] {
            right[label] = x;
        }
        if y > bot[label] {
            bot[label] = y;
        }
        area[label] += 1;
        if br > max_b[label] {
            max_b[label] = br;
        }
    }

    (1..n)
        .filter_map(|i| {
            if area[i] == 0 {
                return None;
            }
            Some((
                i,
                CompStats {
                    left: left[i],
                    top: top[i],
                    width: right[i] - left[i] + 1,
                    height: bot[i] - top[i] + 1,
                    area: area[i],
                    max_brightness: max_b[i],
                },
            ))
        })
        .collect()
}

/// Binarize a grayscale image: non-zero → 255, zero → 0.
/// imageproc::connected_components groups by identical pixel value, so we must
/// collapse all foreground intensities to a single value before labeling.
fn binarize(seg: &GrayImage) -> GrayImage {
    ImageBuffer::from_fn(seg.width(), seg.height(), |x, y| {
        Luma([if seg.get_pixel(x, y).0[0] > 0 {
            255u8
        } else {
            0u8
        }])
    })
}

#[derive(Clone, Copy)]
struct SegmentationParams {
    grey_tol: i16,
    brightness_thresh: u8,
    use_eight: bool,
    min_area: u32,
    /// Pre-sampled background colour from the root box; None means standard (non-overlay) mode.
    overlay_bg: Option<[u8; 3]>,
    allow_yellow: bool,
    ignore_color: bool,
    ui_scale: f64,
}

/// Helper function to find vertical valleys in a wide blob's projection profile.
/// Returns a tuple of (perfect_valley, fallback_valley).
fn find_split_valleys(
    binary: &GrayImage,
    w: u32,
    h: u32,
    ui_scale: f64,
) -> (Option<u32>, Option<u32>) {
    let max_allowed_val = (0.3 * h as f32) as u32;
    let max_allowed_val = if max_allowed_val < 2 { 2 } else { max_allowed_val };

    let min_dist = (3.0 * ui_scale) as i32;
    let min_dist = if min_dist < 2 { 2 } else { min_dist as u32 };

    let mut proj = vec![0u32; w as usize];
    for cx in 0..w {
        let mut col_sum = 0;
        for cy in 0..h {
            if binary.get_pixel(cx, cy).0[0] > 0 {
                col_sum += 1;
            }
        }
        proj[cx as usize] = col_sum;
    }

    let mut perfect_valleys = vec![];
    let mut regular_valleys = vec![];

    if w > min_dist * 2 {
        for cx in min_dist..(w - min_dist) {
            let val = proj[cx as usize];
            let is_local_min =
                val <= proj[(cx - 1) as usize] && val <= proj[(cx + 1) as usize];

            if is_local_min && val <= max_allowed_val {
                regular_valleys.push((cx, val));

                if w > h + 1 {
                    let w_left = cx;
                    let w_right = w - cx;
                    let num_wide = (if w_left + 2 > h { 1 } else { 0 })
                        + (if w_right + 2 > h { 1 } else { 0 });

                    let min_w = w_left.min(w_right) as f32;
                    let max_w = w_left.max(w_right) as f32;
                    let balance = min_w / max_w;

                    if num_wide == 0 && balance >= 0.7 {
                        perfect_valleys.push((cx, val));
                    }
                }
            }
        }
    }

    let perfect_valley = if !perfect_valleys.is_empty() {
        perfect_valleys.sort_by_key(|&(_, val)| val);
        Some(perfect_valleys[0].0)
    } else {
        None
    };

    let fallback_valley = if !regular_valleys.is_empty() {
        let mut best_valley = None;
        let mut best_num_wide = 3;
        let mut best_proj_val = 999999;

        for &(v, proj_val) in &regular_valleys {
            let w_left = v;
            let w_right = w - v;
            let num_wide = (if w_left + 2 > h { 1 } else { 0 })
                + (if w_right + 2 > h { 1 } else { 0 });

            if num_wide < best_num_wide {
                best_num_wide = num_wide;
                best_valley = Some(v);
                best_proj_val = proj_val;
            } else if num_wide == best_num_wide {
                if proj_val < best_proj_val {
                    best_valley = Some(v);
                    best_proj_val = proj_val;
                }
            }
        }
        best_valley
    } else {
        None
    };

    (perfect_valley, fallback_valley)
}

/// Recursive connected-components segmentation.
/// Returns bounding boxes in the coordinate space of the *original* box_img
/// (offsets accumulate via offset_x / offset_y).
fn get_components_recursive(
    roi: &RgbImage,
    params: SegmentationParams,
    offset_x: u32,
    offset_y: u32,
) -> Vec<BoundingBox> {
    let seg = apply_base_filter(
        roi,
        params.grey_tol,
        params.brightness_thresh,
        params.overlay_bg,
        params.allow_yellow,
        params.ignore_color,
    );
    let binary = binarize(&seg);
    let conn = if params.use_eight {
        Connectivity::Eight
    } else {
        Connectivity::Four
    };
    let labeled = connected_components(&binary, conn, Luma([0u8]));

    let max_label = labeled.pixels().map(|p| p.0[0]).max().unwrap_or(0);
    if max_label == 0 {
        return vec![];
    }

    let all_stats = compute_stats(&labeled, &seg, max_label);
    let req_bright = if params.ignore_color {
        150
    } else {
        SEG_REQUIRED_BRIGHTNESS
    };

    let valid: Vec<(usize, CompStats)> = all_stats
        .into_iter()
        .filter(|(_, s)| s.area >= params.min_area && s.max_brightness >= req_bright)
        .collect();

    if valid.is_empty() {
        return vec![];
    }

    let (roi_w, roi_h) = roi.dimensions();

    // Case A: single wide blob that fills the entire ROI → try harder to split.
    if valid.len() == 1 {
        let (_, ref s) = valid[0];
        if s.width == roi_w && s.height == roi_h && s.width + 2 > s.height {
            let (perfect_valley, fallback_valley) =
                find_split_valleys(&binary, s.width, s.height, params.ui_scale);

            let do_split = |v: u32, mut p: SegmentationParams| {
                let mut split_roi = roi.clone();
                for cy in 0..s.height {
                    split_roi.put_pixel(v, cy, image::Rgb([0, 0, 0]));
                }
                p.grey_tol = SEG_GREY_TOLERANCE;
                p.brightness_thresh = SEG_BRIGHTNESS_THRESHOLD;
                p.use_eight = true;
                get_components_recursive(&split_roi, p, offset_x, offset_y)
            };

            if let Some(v) = perfect_valley {
                return do_split(v, params);
            }

            if params.brightness_thresh < 160 {
                let mut p = params;
                p.brightness_thresh += 10;
                return get_components_recursive(roi, p, offset_x, offset_y);
            }
            if params.grey_tol > 2 {
                let mut p = params;
                p.grey_tol -= 2;
                return get_components_recursive(roi, p, offset_x, offset_y);
            }

            if let Some(v) = fallback_valley {
                return do_split(v, params);
            }

            if params.use_eight {
                let mut p = params;
                p.use_eight = false;
                return get_components_recursive(roi, p, offset_x, offset_y);
            }

            // All strategies exhausted: return as a single digit.
            return vec![BoundingBox {
                x: offset_x,
                y: offset_y,
                w: s.width,
                h: s.height,
            }];
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
                    if gx < roi_w
                        && gy < roi_h
                        && labeled.get_pixel(gx, gy).0[0] as usize == *label_idx
                    {
                        sub.put_pixel(px, py, *roi.get_pixel(gx, gy));
                    }
                }
            }
            let sub_boxes =
                get_components_recursive(&sub, params, offset_x + s.left, offset_y + s.top);
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
    overlay_bg: Option<[u8; 3]>,
    allow_yellow: bool,
    ignore_color: bool,
) -> Vec<GrayImage> {
    let min_area = ((BASELINE_MIN_AREA * ui_scale * ui_scale) as u32).max(1);

    let params = SegmentationParams {
        grey_tol: SEG_GREY_TOLERANCE,
        brightness_thresh: SEG_BRIGHTNESS_THRESHOLD,
        use_eight: true,
        min_area,
        overlay_bg,
        allow_yellow,
        ignore_color,
        ui_scale,
    };

    let mut digit_boxes = get_components_recursive(box_img, params, 0, 0);

    digit_boxes.sort_by_key(|d| d.x);

    let mut merged_boxes: Vec<BoundingBox> = Vec::new();
    for db in digit_boxes {
        if let Some(prev) = merged_boxes.last_mut() {
            if db.x < prev.x + prev.w {
                let new_x = prev.x.min(db.x);
                let new_y = prev.y.min(db.y);
                let new_right = (prev.x + prev.w).max(db.x + db.w);
                let new_bottom = (prev.y + prev.h).max(db.y + db.h);
                let new_w = new_right - new_x;
                let new_h = new_bottom - new_y;

                if new_w + 2 <= new_h {
                    prev.x = new_x;
                    prev.y = new_y;
                    prev.w = new_w;
                    prev.h = new_h;
                    continue;
                }
            }
        }
        merged_boxes.push(db);
    }

    let (ow, oh) = out_img.dimensions();
    merged_boxes
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

pub fn detect_baseline(
    box_img: &RgbImage,
    ui_scale: f64,
    ignore_color: bool,
) -> Option<(u32, u32)> {
    let min_area = ((BASELINE_MIN_AREA * ui_scale * ui_scale) as u32).max(1);

    let params = SegmentationParams {
        grey_tol: SEG_GREY_TOLERANCE,
        brightness_thresh: SEG_BRIGHTNESS_THRESHOLD,
        use_eight: true,
        min_area,
        overlay_bg: None,
        allow_yellow: false,
        ignore_color,
        ui_scale,
    };

    let boxes = get_components_recursive(box_img, params, 0, 0);

    if boxes.is_empty() {
        return None;
    }

    let y_min = boxes.iter().map(|b| b.y).min().unwrap_or(0);
    let y_max = boxes.iter().map(|b| b.y + b.h).max().unwrap_or(0);
    Some((y_max - y_min, y_min))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn test_binarize() {
        let mut img = GrayImage::new(2, 2);
        img.put_pixel(0, 0, Luma([100]));
        img.put_pixel(1, 1, Luma([0]));

        let b = binarize(&img);
        assert_eq!(b.get_pixel(0, 0).0[0], 255);
        assert_eq!(b.get_pixel(1, 1).0[0], 0);
    }

    #[test]
    fn test_segmentation_simple() {
        // Create an image with two separate white boxes on a black background
        let mut img = RgbImage::new(20, 10);

        // Box 1: (2,2) to (4,4)
        for y in 2..5 {
            for x in 2..5 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }

        // Box 2: (10,2) to (12,4)
        for y in 2..5 {
            for x in 10..13 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }

        let params = SegmentationParams {
            grey_tol: 10,
            brightness_thresh: 128,
            use_eight: true,
            min_area: 1,
            overlay_bg: None,
            allow_yellow: false,
            ignore_color: false,
            ui_scale: 1.0,
        };
        let boxes = get_components_recursive(&img, params, 0, 0);

        assert_eq!(boxes.len(), 2);

        // Sort by x to verify positions
        let mut b = boxes;
        b.sort_by_key(|r| r.x);

        assert_eq!(b[0].x, 2);
        assert_eq!(b[1].x, 10);
    }

    #[test]
    fn test_segment_into_digits_overlay_tiny_roi() {
        // Create an image that triggers the standard processing recursive path
        // for a sub-image that is smaller than 3x3 (e.g., 2x1).
        let mut box_img = RgbImage::new(10, 5);

        // Put bright pixels at (2, 2) and (3, 2).
        // Using a ui_scale of 0.1 ensures min_area evaluates to 1.
        box_img.put_pixel(2, 2, Rgb([255, 255, 255]));
        box_img.put_pixel(3, 2, Rgb([255, 255, 255]));

        let out_img = GrayImage::new(10, 5);
        let bg = [100u8, 100u8, 100u8];

        // Call segment_into_digits with overlay_bg and ui_scale = 0.1 to force min_area to 1
        let digits = segment_into_digits(
            &box_img,
            &out_img,
            0.1,
            Some(bg),
            false,
            false,
        );

        // This should run without panicking.
        let _ = digits;
    }
}
