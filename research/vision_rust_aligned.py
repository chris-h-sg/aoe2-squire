import cv2
import numpy as np
import os
import json
from pathlib import Path
from typing import Dict, Any, List, Optional

# ==========================================
# CONSTANTS (Synced with src/constants.rs)
# ==========================================
BASELINE_MARGIN = 263
RED_R_MIN = 160
RED_G_MAX = 70
RED_B_MAX = 70
RED_PIXEL_MIN_COUNT = 12
ANCHOR_MIN_X = 0.5
ANCHOR_MAX_X = 0.95
ANCHOR_MIN_Y = 0.015
ANCHOR_MAX_Y = 0.05

OUT_GREY_TOLERANCE = 30
OUT_BRIGHTNESS_THRESHOLD = 5
OUT_OVERLAY_BRIGHTNESS_THRESHOLD = 30

SEG_GREY_TOLERANCE = 30
SEG_BRIGHTNESS_THRESHOLD = 100
SEG_REQUIRED_BRIGHTNESS = 230
BASELINE_MIN_AREA = 15.0

WORKING_HEIGHT = 36
CANVAS_SIZE = 64
BLUR_SIGMA = 1.0
WIGGLE_OFFSETS = [-1, 0, 1]

# ==========================================
# 1. ROBUST SCALE DETECTION (Backport from anchor.rs)
# ==========================================

def detect_ui_scale(img_bgr, baseline_margin=BASELINE_MARGIN):
    if img_bgr is None:
        return None

    h, w, _ = img_bgr.shape
    
    # Calculate scan boundaries
    y_min = int(h * ANCHOR_MIN_Y)
    y_max = int(h * ANCHOR_MAX_Y)
    x_min_scan = int(w * ANCHOR_MIN_X)
    x_max_scan = int(w * ANCHOR_MAX_X)
    
    # Pre-filter for red pixels to find potential anchors quickly
    # OpenCV uses BGR, but we want RGB for the comparison logic
    img_rgb = cv2.cvtColor(img_bgr[y_min:y_max, x_min_scan:x_max_scan], cv2.COLOR_BGR2RGB)
    
    scan_h, scan_w, _ = img_rgb.shape
    best_rightmost_x = None
    
    # Stage 1: Fast scan for red clusters
    for y in range(scan_h):
        for x in range(scan_w):
            r, g, b = img_rgb[y, x]
            if r >= RED_R_MIN and g < RED_G_MAX and b < RED_B_MAX:
                # Density check: count red pixels in a 10x10 box extending to the left
                count = 0
                x_min = max(0, x - 9)
                y_max_scan = min(scan_h - 1, y + 9)
                
                # Check a 10x10 area
                sub_area = img_rgb[y:y_max_scan+1, x_min:x+1]
                r_mask = (sub_area[:,:,0] >= RED_R_MIN) & \
                         (sub_area[:,:,1] < RED_G_MAX) & \
                         (sub_area[:,:,2] < RED_B_MAX)
                count = np.sum(r_mask)

                if count >= RED_PIXEL_MIN_COUNT:
                    if best_rightmost_x is None or x > best_rightmost_x:
                        best_rightmost_x = x

    if best_rightmost_x is None:
        return None
        
    # Return absolute X coordinate (accounting for scan offset)
    abs_x = best_rightmost_x + x_min_scan
    margin_px = w - abs_x
    return margin_px / baseline_margin

# ==========================================
# 2. FILTERING & SEGMENTATION (Backport from filter.rs)
# ==========================================

def apply_base_filter(img_bgr, grey_tol, brightness_thresh, overlay_mode=False, allow_yellow=False):
    h, w, _ = img_bgr.shape
    img_rgb = cv2.cvtColor(img_bgr, cv2.COLOR_BGR2RGB).astype(np.int16)

    if overlay_mode:
        bg = img_rgb[2, 2]
        # Subtract background, convert to manual luminance
        diff = np.clip(img_rgb - bg, 0, 255).astype(np.float32)
        lumas = (0.299 * diff[:,:,0] + 0.587 * diff[:,:,1] + 0.114 * diff[:,:,2])
        
        lo = np.min(lumas)
        hi = np.max(lumas)
        range_val = hi - lo
        
        if range_val > 0:
            norm = ((lumas - lo) / range_val * 255.0)
        else:
            norm = np.zeros_like(lumas)
            
        out = np.where(norm >= brightness_thresh, norm, 0).astype(np.uint8)
        return out

    # Standard mode
    r, g, b = img_rgb[:,:,0], img_rgb[:,:,1], img_rgb[:,:,2]
    max_ch = np.max(img_rgb, axis=2)
    min_ch = np.min(img_rgb, axis=2)
    
    keep = (max_ch - min_ch) <= grey_tol
    
    if allow_yellow:
        rg_diff = np.abs(r - g)
        is_yellow = (r > 150) & (g > 150) & (rg_diff < 50) & (b < max_ch - 15)
        keep |= is_yellow
        
    out = np.where(keep & (max_ch >= brightness_thresh), max_ch, 0).astype(np.uint8)
    return out

def step3_segment_into_digits(box_img_bgr, out_img_grey, ui_scale=1.0, overlay_mode=False, allow_yellow=False):
    # Same recursive logic as poc_vision, but ensures it uses the Rust-aligned filter
    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    
    # We use cv2 for connectivity but the filter must be the one above
    def get_components(roi_bgr, grey_tolerance, brightness_threshold, offset_x=0, offset_y=0):
        seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow)
        binary = (seg > 0).astype(np.uint8)
        num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=8)
        
        results = []
        for i in range(1, num_labels):
            if stats[i, cv2.CC_STAT_AREA] >= min_area:
                x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
                # Required brightness check
                if np.max(seg[y:y+h, x:x+w][labels[y:y+h, x:x+w] == i]) >= SEG_REQUIRED_BRIGHTNESS:
                    if w + 2 > h: # Potential merged digit
                        # In Rust we recurse, here we just take the box for now to match simplest logic
                        results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
                    else:
                        results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
        return results

    digit_boxes = get_components(box_img_bgr, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD)
    digit_boxes.sort(key=lambda d: d['x'])
    
    digits = []
    for db in digit_boxes:
        crop = out_img_grey[db['y'] : db['y']+db['h'], db['x'] : db['x']+db['w']]
        digits.append(crop)
    return digits, digit_boxes

# ==========================================
# 3. MATCHING (Backport from canvas.rs & matcher.rs)
# ==========================================

def prepare_canvas(img_grey):
    """Backport of Stage 6: resize -> center -> blur -> f32."""
    h_orig, w_orig = img_grey.shape
    if h_orig == 0 or w_orig == 0:
        return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)

    # 1. Scale height to WORKING_HEIGHT
    scale = WORKING_HEIGHT / h_orig
    new_w = max(1, int(w_orig * scale))
    
    # Use cv2.resize with specific filters to match Rust 'image' crate
    if scale > 1.0:
        # CatmullRom in Rust is a cubic filter. 
        # cv2.INTER_CUBIC is the closest standard equivalent in OpenCV.
        interpolation = cv2.INTER_CUBIC
    else:
        # Triangle in Rust is a BILINEAR filter.
        interpolation = cv2.INTER_LINEAR
        
    upscaled = cv2.resize(img_grey, (new_w, WORKING_HEIGHT), interpolation=interpolation)
    
    # 2. Bounding Box check (value > 1)
    coords = np.argwhere(upscaled > 1)
    if coords.size == 0:
        return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)
    
    y_min, x_min = coords.min(axis=0)
    y_max, x_max = coords.max(axis=0)
    crop = upscaled[y_min:y_max+1, x_min:x_max+1]
    
    ch, cw = crop.shape
    
    # Safety: resize down if the crop is somehow larger than the canvas (matches Rust)
    if cw > CANVAS_SIZE or ch > CANVAS_SIZE:
        fit_scale = min(CANVAS_SIZE / cw, CANVAS_SIZE / ch)
        fw = max(1, int(cw * fit_scale))
        fh = max(1, int(ch * fit_scale))
        crop = cv2.resize(crop, (fw, fh), interpolation=cv2.INTER_LINEAR)
        ch, cw = crop.shape

    # 3. Center in 64x64
    canvas = np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.uint8)
    off_x = (CANVAS_SIZE - cw) // 2
    off_y = (CANVAS_SIZE - ch) // 2
    
    # Overlay
    canvas[off_y:off_y+ch, off_x:off_x+cw] = crop
    
    # 4. Blur and convert to float
    canvas_f = canvas.astype(np.float32) / 255.0
    # cv2.GaussianBlur uses sigma, we must match the kernel size (approx 0 = auto)
    blurred = cv2.GaussianBlur(canvas_f, (0, 0), BLUR_SIGMA)
    return blurred

def match_digit_to_template(digit_img, processed_templates):
    input_canvas = prepare_canvas(digit_img)
    size = CANVAS_SIZE
    
    # Pre-compute 9 shifted versions (pixel-perfect manual shift like Rust)
    shifted_inputs = []
    for dy in WIGGLE_OFFSETS:
        for dx in WIGGLE_OFFSETS:
            shifted = np.zeros((size, size), dtype=np.float32)
            # Map input[y+dy, x+dx] to shifted[y, x]
            y_start, y_end = max(0, -dy), min(size, size - dy)
            x_start, x_end = max(0, -dx), min(size, size - dx)
            
            sy_start, sy_end = max(0, dy), min(size, size + dy)
            sx_start, sx_end = max(0, dx), min(size, size + dx)
            
            shifted[y_start:y_end, x_start:x_end] = input_canvas[sy_start:sy_end, sx_start:sx_end]
            shifted_inputs.append(shifted)

    all_matches = []
    for char, template_canvas in processed_templates.items():
        best_ssd = float('inf')
        for shifted_input in shifted_inputs:
            diff = shifted_input - template_canvas
            ssd = np.sum(diff * diff)
            best_ssd = min(best_ssd, ssd)
        all_matches.append((best_ssd, char))
    
    all_matches.sort()
    
    # Tie-breaker (Sync with matcher.rs)
    if len(all_matches) > 1:
        best_ssd, best_char = all_matches[0]
        second_ssd, second_char = all_matches[1]
        asym = ['3', '6', '9']
        is_conflict = (best_char == '0' and second_char in asym) or \
                      (best_char in asym and second_char == '0')
        
        if is_conflict and second_ssd - best_ssd < best_ssd * 0.20:
            # Symmetry score
            flipped = np.flip(input_canvas, axis=1)
            diff = input_canvas - flipped
            sym_score = np.sum(diff * diff)
            
            if (best_char == '0' and sym_score > 60.0) or \
               (best_char in asym and sym_score < 40.0):
                all_matches[0], all_matches[1] = all_matches[1], all_matches[0]

    return all_matches[0][0], all_matches[0][1]

# ==========================================
# 4. MAIN ENTRY (Wrapper for poc_video/extract_outliers)
# ==========================================

def process_frame(img_bgr, ui_map, templates, verbose=True):
    if img_bgr is None:
        return None

    # 1. Detect Scale
    ui_scale = detect_ui_scale(img_bgr)
    if ui_scale is None:
        if verbose:
            print("[no anchor] Red UI reference not found — game not visible or UI changed.")
        return {} # Return empty dict instead of None to avoid iteration errors
    
    # Scale safety check: 0.8x to 1.25x baseline
    if ui_scale < 0.8 or ui_scale > 1.25:
        if verbose:
            print(f"[warning] Suspicious UI scale detected: {ui_scale:.2f}")

    results = {}
    pop_color = "white"
    
    for name, coords in ui_map.get('elements', {}).items():
        x = int(coords['x_px'] * ui_scale)
        y = int(coords['y_px'] * ui_scale)
        w = int(coords['w_px'] * ui_scale)
        h = int(coords['h_px'] * ui_scale)
        
        if x+w > img_bgr.shape[1] or y+h > img_bgr.shape[0]:
            continue
            
        box_img = img_bgr[y:y+h, x:x+w]
        overlay_mode = False
        allow_yellow = False

        if name == "population_total":
            # detect_housed_overlay backport
            npx = box_img.shape[0] * box_img.shape[1]
            if npx > 0:
                mean_b, mean_g, mean_r = np.mean(box_img, axis=(0,1))
                overlay_mode = mean_r > 150 and mean_g > 150 and mean_b < 100
            
            allow_yellow = True
            if overlay_mode:
                pop_color = "overlay"
            elif contains_yellow(box_img):
                pop_color = "yellow"

        # Special Case: Idle
        if name == "idle_vils" and not contains_yellow(box_img):
            value_str = "0"
        else:
            out_img = apply_base_filter(box_img, OUT_GREY_TOLERANCE, 
                                       OUT_OVERLAY_BRIGHTNESS_THRESHOLD if overlay_mode else OUT_BRIGHTNESS_THRESHOLD,
                                       overlay_mode, allow_yellow)
            digits, _ = step3_segment_into_digits(box_img, out_img, ui_scale, overlay_mode, allow_yellow)
            
            value_str = ""
            for d_img in digits:
                _, char = match_digit_to_template(d_img, templates)
                value_str += char
        
        parts = name.split('_')
        category, sub_key = parts[0], parts[1] if len(parts) > 1 else "value"
        if category not in results: results[category] = {}
        
        if name == "population_total":
            if "/" in value_str:
                curr, housing = value_str.split('/', 1)
                results[category]['total'], results[category]['housing'] = curr, housing
            else:
                results[category]['total'], results[category]['housing'] = value_str, ""
        else:
            results[category][sub_key] = value_str
            
    if 'population' not in results: results['population'] = {}
    results['population']['color'] = pop_color
    return results

def contains_yellow(img_bgr):
    # R>100, G>100, B < min(R,G)-50, |R-G| < 50
    img_rgb = cv2.cvtColor(img_bgr, cv2.COLOR_BGR2RGB).astype(np.int16)
    r, g, b = img_rgb[:,:,0], img_rgb[:,:,1], img_rgb[:,:,2]
    is_yellow = (r > 100) & (g > 100) & (b < np.minimum(r, g) - 50) & (np.abs(r - g) < 50)
    return np.any(is_yellow)

def load_templates(templates_dir):
    # We must use the Rust-aligned prepare_canvas
    templates = {}
    if not os.path.exists(templates_dir):
        return templates
    for filename in os.listdir(templates_dir):
        if filename.endswith(".png"):
            char = filename.replace(".png", "")
            if char == "slash": char = "/"
            img = cv2.imread(os.path.join(templates_dir, filename), cv2.IMREAD_GRAYSCALE)
            if img is not None:
                templates[char] = prepare_canvas(img)
    return templates

def main():
    import argparse
    parser = argparse.ArgumentParser(description="Test Rust-aligned vision on a single image.")
    parser.add_argument("image", type=str, help="Path to the image file")
    parser.add_argument("--show", action="store_true", help="Show the processed box for a specific element")
    parser.add_argument("--key", type=str, default="population_vils", help="The UI element key to show (default: population_vils)")
    args = parser.parse_args()

    img = cv2.imread(args.image)
    if img is None:
        print(f"Error: Could not load image {args.image}")
        return

    script_dir = Path(__file__).parent.absolute()
    
    # Load UI Map
    map_path = script_dir.parent / "ui_map.json"
    with open(map_path, 'r') as f:
        ui_map = json.load(f)

    # Load Templates
    templates_dir = script_dir / "templates" / "enormous_numbers"
    templates = load_templates(str(templates_dir))

    # --- 1. Scale Detection ---
    h, w, _ = img.shape
    top_h = max(1, int(h * ANCHOR_SCAN_FRACTION))
    img_rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
    
    best_rightmost_x = None
    anchor_y = None

    for y in range(top_h):
        for x in range(w):
            r, g, b = img_rgb[y, x]
            if r >= RED_R_MIN and g < RED_G_MAX and b < RED_B_MAX:
                count = 0
                x_min = max(0, x - 9)
                y_max = min(top_h - 1, y + 9)
                sub_area = img_rgb[y:y_max+1, x_min:x+1]
                r_mask = (sub_area[:,:,0] >= RED_R_MIN) & (sub_area[:,:,1] < RED_G_MAX) & (sub_area[:,:,2] < RED_B_MAX)
                count = np.sum(r_mask)
                if count >= RED_PIXEL_MIN_COUNT:
                    if best_rightmost_x is None or x > best_rightmost_x:
                        best_rightmost_x = x
                        anchor_y = y

    if best_rightmost_x is None:
        print("ANCHOR: Not Found")
        if args.show:
            # Show the top area where we search for the anchor
            search_area = img[0:top_h, :]
            cv2.imshow("Anchor Search Area (Top of Screen)", search_area)
            cv2.waitKey(0)
            cv2.destroyAllWindows()
        return

    ui_scale = (w - best_rightmost_x) / BASELINE_MARGIN
    print(f"ANCHOR: x={best_rightmost_x}, y={anchor_y} (pixels)")
    print(f"SCALE:  {ui_scale:.4f}")

    # --- 2. OCR ---
    results = process_frame(img, ui_map, templates, verbose=True)
    
    if results:
        # Display the result for the specific key we're interested in
        parts = args.key.split('_')
        category = parts[0]
        sub_key = parts[1] if len(parts) > 1 else "value"
        
        if args.key == "population_total":
            val = f"{results.get('population', {}).get('total', '')}/{results.get('population', {}).get('housing', '')}"
        else:
            val = results.get(category, {}).get(sub_key, "N/A")
            
        print(f"RESULT ({args.key}): {val}")

    # --- 3. Optional Debug Show ---
    if args.show:
        if args.key not in ui_map['elements']:
            print(f"Error: Key '{args.key}' not found in ui_map.json")
            return

        # Re-extract the specific box to show the filtered version
        coords = ui_map['elements'][args.key]
        x = int(coords['x_px'] * ui_scale)
        y = int(coords['y_px'] * ui_scale)
        w = int(coords['w_px'] * ui_scale)
        h = int(coords['h_px'] * ui_scale)
        box_img = img[y:y+h, x:x+w]
        
        overlay_mode = False
        allow_yellow = False
        if args.key == "population_total":
            mean_b, mean_g, mean_r = np.mean(box_img, axis=(0,1))
            overlay_mode = mean_r > 150 and mean_g > 150 and mean_b < 100
            allow_yellow = True
        
        clean = apply_base_filter(box_img, OUT_GREY_TOLERANCE, 
                                 OUT_OVERLAY_BRIGHTNESS_THRESHOLD if overlay_mode else OUT_BRIGHTNESS_THRESHOLD,
                                 overlay_mode, allow_yellow=allow_yellow)
        
        # Upscale for visibility
        display = cv2.resize(clean, (clean.shape[1]*4, clean.shape[0]*4), interpolation=cv2.INTER_NEAREST)
        cv2.imshow(f"Filtered {args.key} (Scale x4)", display)
        print("\nPress any key in the image window to close...")
        cv2.waitKey(0)
        cv2.destroyAllWindows()

if __name__ == "__main__":
    main()
