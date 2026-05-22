import cv2
import numpy as np
import json
import os
import argparse
import shutil
import time
from pathlib import Path

"""
AoE2 Squire - Extraction Debugger
---------------------------------
This script verifies the segmentation and coordinate mapping logic of the 
vision pipeline.

ARCHITECTURAL PHILOSOPHY: "Detect First, Extract Second"
1. CALIBRATION: Detect UI scale and mod-specific geometry using fixed anchors.
2. ANCHORING: Use a reference box (Wood) to find the vertical baseline of digits.
3. NORMALIZATION: Clip boxes to the detected baseline to ensure digits are vertically centered.
4. ROBUST OCR: Combine anchoring with standard vertical wiggle to maintain 100% accuracy.
"""

# --- Constants: Scale Detection ---
RED_PIXEL_MIN_COUNT = 12
RED_DIFF_THRESHOLD = 140     # Red must be this much higher than Green and Blue
ANCHOR_MIN_X = 0.5
ANCHOR_MAX_X = 0.95
ANCHOR_MIN_Y = 0.015
ANCHOR_MAX_Y = 0.035
UI_SCALE_MIN = 0.5
UI_SCALE_MAX = 2.0

# --- Constants: Mod Support (Anne_HK) ---
ANNE_HK_PIXEL_MIN_COUNT = 12
ANNE_HK_COLOR_DIFF_THRESHOLD = 150  # R and G must be this much higher than B
ANNE_HK_Y_ADJUST_FACTOR = 0.6       # Villager boxes expanded up by 60%
ANNE_HK_IDLE_W_ADJUST_FACTOR = 0.4   # Idle vils box expanded width by 40%
ANNE_HK_IDLE_RED_MIN = 150          # Red indicator threshold
ANNE_HK_IDLE_GB_MAX = 10            # Max Green/Blue for the red indicator

# --- Constants: Segmentation ---
SEG_GREY_TOLERANCE = 30
SEG_BRIGHTNESS_THRESHOLD = 100
BASELINE_MIN_AREA = 15

# --- Constants: Cleanup ---
OUT_GREY_TOLERANCE = 30
OUT_BRIGHTNESS_THRESHOLD_DEFAULT = 5
OUT_BRIGHTNESS_THRESHOLD_OVERLAY = 30

# --- Utility Functions: Detectors ---

def detect_ui_scale(img, baseline_margin=263):
    """Detects the UI scale based on the right-side margin of red UI elements."""
    if img is None:
        return 1.0

    h, w, _ = img.shape
    y_min_scan = int(h * ANCHOR_MIN_Y)
    y_max_scan = min(h - 1, int(h * ANCHOR_MAX_Y))
    x_min_scan = int(w * ANCHOR_MIN_X)
    x_max_scan = min(w - 1, int(w * ANCHOR_MAX_X))
    
    img_rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
    best_rightmost_x = None

    for y in range(y_min_scan, y_max_scan + 1):
        for x in range(x_min_scan, x_max_scan + 1):
            r, g, b = img_rgb[y, x]
            if (int(r) - int(g) >= RED_DIFF_THRESHOLD) and (int(r) - int(b) >= RED_DIFF_THRESHOLD):
                x_start = max(x_min_scan, x - 9)
                y_end = min(y_max_scan, y + 9)
                sub_area = img_rgb[y : y_end + 1, x_start : x + 1]
                
                r_chan = sub_area[:, :, 0].astype(np.int16)
                g_chan = sub_area[:, :, 1].astype(np.int16)
                b_chan = sub_area[:, :, 2].astype(np.int16)
                r_mask = (r_chan - g_chan >= RED_DIFF_THRESHOLD) & (r_chan - b_chan >= RED_DIFF_THRESHOLD)
                
                if np.sum(r_mask) >= RED_PIXEL_MIN_COUNT:
                    if best_rightmost_x is None or x > best_rightmost_x:
                        best_rightmost_x = x

    if best_rightmost_x is None:
        return 1.0
        
    margin_px = w - best_rightmost_x
    ui_scale = margin_px / baseline_margin
    return ui_scale if UI_SCALE_MIN <= ui_scale <= UI_SCALE_MAX else 1.0

def detect_anne_hk_mod(full_img, x, y, w, h):
    """Detects 'Anne_HK resource panels' mod using a two-stage color matching and bounding box expansion approach."""
    if full_img is None or len(full_img.shape) < 3:
        return False
    default_box_img = full_img[y:y+h, x:x+w]
    if default_box_img is None or len(default_box_img.shape) < 3:
        return False
    
    b, g, r = default_box_img[:, :, 0].astype(np.int16), default_box_img[:, :, 1].astype(np.int16), default_box_img[:, :, 2].astype(np.int16)
    mask = (r - b > ANNE_HK_COLOR_DIFF_THRESHOLD) & (g - b > ANNE_HK_COLOR_DIFF_THRESHOLD)
    if not np.any(mask):
        return False
        
    # Check the adjusted box to see if it meets the min count
    y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
    y_adj_start = max(0, y - y_adj)
    h_adj_len = h + y_adj
    adjusted_box_img = full_img[y_adj_start:y_adj_start+h_adj_len, x:x+w]
    
    b_adj, g_adj, r_adj = adjusted_box_img[:, :, 0].astype(np.int16), adjusted_box_img[:, :, 1].astype(np.int16), adjusted_box_img[:, :, 2].astype(np.int16)
    mask_adj = (r_adj - b_adj > ANNE_HK_COLOR_DIFF_THRESHOLD) & (g_adj - b_adj > ANNE_HK_COLOR_DIFF_THRESHOLD)
    return np.sum(mask_adj) >= ANNE_HK_PIXEL_MIN_COUNT

def detect_housed_overlay(img):
    """Detects the bright yellow background overlay used when a player is housed."""
    if img is None or len(img.shape) < 3:
        return False
    return np.mean(img[:, :, 1]) > 150 and np.mean(img[:, :, 2]) > 150 and np.mean(img[:, :, 0]) < 100

def contains_yellow(img, min_brightness=100, blue_margin=50, rg_similarity=50):
    """Checks if the box contains yellow pixels (active idle vils icon)."""
    if img is None or len(img.shape) < 3: return False
    b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    is_yellow = (g > min_brightness) & (r > min_brightness) & \
                (b < (g.astype(np.int16) - blue_margin)) & \
                (b < (r.astype(np.int16) - blue_margin)) & \
                (np.abs(r.astype(np.int16) - g.astype(np.int16)) < rg_similarity)
    return np.any(is_yellow)

def contains_red(img):
    """Checks if the box contains red pixels (Anne_HK mod idle vils indicator)."""
    if img is None or len(img.shape) < 3: return False
    b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    is_red = (r > ANNE_HK_IDLE_RED_MIN) & (g < ANNE_HK_IDLE_GB_MAX) & (b < ANNE_HK_IDLE_GB_MAX)
    return np.any(is_red)

# --- Utility Functions: Image Processing ---

def apply_base_filter(img, grey_tol, brightness_thresh, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """Filters image by greyness and brightness. Supports overlay subtraction and yellow detection."""
    if ignore_color:
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        _, cleaned = cv2.threshold(gray, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if overlay_mode:
        bg_color = img[2, 2].astype(np.int16)
        subtracted = np.clip(img.astype(np.int16) - bg_color, 0, 255).astype(np.uint8)
        gray_sub = cv2.cvtColor(subtracted, cv2.COLOR_BGR2GRAY)
        normalized = cv2.normalize(gray_sub, None, 0, 255, cv2.NORM_MINMAX)
        _, cleaned = cv2.threshold(normalized, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if len(img.shape) == 3:
        b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
        max_val = np.max(img, axis=2).astype(np.int16)
        min_val = np.min(img, axis=2).astype(np.int16)
        to_keep = (max_val - min_val <= grey_tol)
        
        if allow_yellow:
            is_yellow = (r > 150) & (g > 150) & (np.abs(r.astype(np.int16) - g.astype(np.int16)) < 50) & (b < max_val - 15)
            to_keep |= is_yellow
        
        filtered = np.zeros(max_val.shape, dtype=np.uint8)
        filtered[to_keep] = max_val.astype(np.uint8)[to_keep]
    else:
        filtered = img

    _, cleaned = cv2.threshold(filtered, brightness_thresh, 255, cv2.THRESH_TOZERO)
    return cleaned

def cleanup_box(box_img, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """Performs final 'soft' filtering for output images."""
    thresh = OUT_BRIGHTNESS_THRESHOLD_OVERLAY if overlay_mode else OUT_BRIGHTNESS_THRESHOLD_DEFAULT
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, thresh, overlay_mode, allow_yellow, ignore_color)

# --- Segmentation Engine ---

def get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold, 
                             overlay_mode=False, allow_yellow=False, ignore_color=False, 
                             offset_x=0, offset_y=0, connectivity=8, required_brightness=230):
    """Finds connected components, recursing to split wide blobs."""
    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    req_bright = required_brightness if not ignore_color else 150
    
    seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color)
    binary = (seg > 0).astype(np.uint8)
    num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
    
    valid_indices = []
    for i in range(1, num_labels):
        if stats[i, cv2.CC_STAT_AREA] >= min_area:
            x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
            if np.max(seg[y:y+h, x:x+w][labels[y:y+h, x:x+w] == i]) >= req_bright:
                valid_indices.append(i)

    if not valid_indices: return []

    img_h, img_w = roi_bgr.shape[:2]
    if len(valid_indices) == 1:
        idx = valid_indices[0]
        w, h = stats[idx, cv2.CC_STAT_WIDTH], stats[idx, cv2.CC_STAT_HEIGHT]
        if w == img_w and h == img_h and (w + 2 > h):
            max_allowed_val = max(2, int(0.3 * h))
            min_dist = max(2, int(3 * ui_scale))
            proj = np.sum(binary, axis=0)

            perfect_valleys = []
            regular_valleys = []

            for cx in range(min_dist, w - min_dist):
                val = proj[cx]
                is_local_min = val <= proj[cx - 1] and val <= proj[cx + 1]
                if is_local_min and val <= max_allowed_val:
                    regular_valleys.append((cx, val))
                    if w > h + 1:
                        w_left = cx
                        w_right = w - cx
                        num_wide = (1 if w_left + 2 > h else 0) + (1 if w_right + 2 > h else 0)
                        balance = min(w_left, w_right) / max(w_left, w_right)
                        if num_wide == 0 and balance >= 0.7:  # neither half is less than 70% of the other
                            perfect_valleys.append((cx, val))

            if perfect_valleys:
                perfect_valleys.sort(key=lambda item: item[1])
                best_valley = perfect_valleys[0][0]
                split_roi = roi_bgr.copy()
                split_roi[:, best_valley] = 0
                return get_components_recursive(split_roi, ui_scale, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)

            if brightness_threshold < 160:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold + 10, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)
            if grey_tolerance > 2:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance - 2, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)

            # Valley Splitting: Try to split the wide merged component vertically using the regular valleys
            if regular_valleys:
                # Select the single best valley by minimising the number of resulting wide parts
                best_valley = None
                best_num_wide = 3  # Greater than max possible outcome (2)
                best_proj_val = 999999

                for cx, val in regular_valleys:
                    w_left = cx
                    w_right = w - cx
                    num_wide = (1 if w_left + 2 > h else 0) + (1 if w_right + 2 > h else 0)

                    if num_wide < best_num_wide:
                        best_num_wide = num_wide
                        best_valley = cx
                        best_proj_val = val
                    elif num_wide == best_num_wide:
                        # Tie-breaker: prefer the deeper valley (lower projection count)
                        if val < best_proj_val:
                            best_valley = cx
                            best_proj_val = val

                if best_valley is not None:
                    split_roi = roi_bgr.copy()
                    split_roi[:, best_valley] = 0
                    # Recurse on split ROI, resetting thresholding parameters to their high-quality defaults
                    return get_components_recursive(split_roi, ui_scale, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)
            
            # Fallback to 4-connectivity as a last resort
            if connectivity == 8:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, 4, required_brightness)
            return [{'x': offset_x, 'y': offset_y, 'w': w, 'h': h}]

    results = []
    for i in valid_indices:
        x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
        if w + 2 > h:
            isolated_roi = roi_bgr[y:y+h, x:x+w].copy()
            isolated_roi[labels[y:y+h, x:x+w] != i] = 0
            results.extend(get_components_recursive(isolated_roi, ui_scale, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x + x, offset_y + y, connectivity, required_brightness))
        else:
            results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
            
    return results

def segment_box(box_img, out_img, ui_scale, overlay_mode=False, bright_threshold=230, allow_yellow=False, ignore_color=False, force_full_height=False):
    """Segments a box into digit crops."""
    boxes = get_components_recursive(box_img, ui_scale, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD, 
                                     overlay_mode, allow_yellow, ignore_color, required_brightness=bright_threshold)
    boxes.sort(key=lambda d: d['x'])
    
    # Post-processing: Merge horizontally overlapping components
    # This prevents vertically fractured characters (like the top and bottom of a '2') 
    # from being treated as separate digits.
    merged_boxes = []
    for db in boxes:
        if not merged_boxes:
            merged_boxes.append(db)
            continue
            
        prev = merged_boxes[-1]
        # Check for horizontal overlap (strict)
        if db['x'] < prev['x'] + prev['w']:
            # Calculate potential merged dimensions
            new_x = min(prev['x'], db['x'])
            new_y = min(prev['y'], db['y'])
            new_right = max(prev['x'] + prev['w'], db['x'] + db['w'])
            new_bottom = max(prev['y'] + prev['h'], db['y'] + db['h'])
            new_w = new_right - new_x
            new_h = new_bottom - new_y
            
            # Only merge if the combined box is not considered "wide" (multiple digits)
            if new_w + 2 <= new_h:
                merged_boxes[-1] = {
                    'x': new_x,
                    'y': new_y,
                    'w': new_w,
                    'h': new_h
                }
            else:
                merged_boxes.append(db)
        else:
            merged_boxes.append(db)
            
    boxes = merged_boxes
    
    digits = []
    h_img = out_img.shape[0]
    for db in boxes:
        y = 0 if force_full_height else db['y']
        h = h_img if force_full_height else db['h']
        digits.append(out_img[y : y+h, db['x'] : db['x']+db['w']])
        
    print(f"  Segmentation: found {len(digits)} components.")
    return digits, boxes

# --- Core Pipeline Class ---

class ExtractorPipeline:
    def __init__(self, ui_map, bright_threshold=230):
        self.ui_map = ui_map
        self.bright_thresh = bright_threshold
        self.ui_scale = 1.0
        self.anne_hk_active = False
        self.ref_h = None
        self.ref_y = None
        self.vil_ref_h = None
        self.vil_ref_y = None

    def calibrate(self, img):
        """
        Calibrates the pipeline by detecting UI scale, active mods, and reference 
        digit metrics (height and Y-offset) from the current image.
        """
        self.ui_scale = detect_ui_scale(img, self.ui_map.get('baseline_margin', 263))
        
        # Mod detection
        wood_vils_coords = self.ui_map.get('elements', {}).get('wood_vils')
        if wood_vils_coords:
            x, y, w, h = self._get_raw_coords("wood_vils", wood_vils_coords)
            if detect_anne_hk_mod(img, x, y, w, h):
                self.anne_hk_active = True
                print("    [Anne_HK resource panels] mod detected!")
        
        # Reference metrics from wood_total (Primary Resource Digits)
        wood_total_coords = self.ui_map.get('elements', {}).get('wood_total')
        if wood_total_coords:
            self.ref_h, self.ref_y = self._detect_baseline(img, "wood_total", wood_total_coords)
            if self.ref_h:
                print(f"Reference digit height set to: {self.ref_h} (offset {self.ref_y}) from wood_total")

        # Reference metrics from wood_vils (Villager Digits)
        if wood_vils_coords:
            self.vil_ref_h, self.vil_ref_y = self._detect_baseline(img, "wood_vils", wood_vils_coords)
            if self.vil_ref_h:
                print(f"Reference villager height set to: {self.vil_ref_h} (offset {self.vil_ref_y}) from wood_vils")

    def _detect_baseline(self, img, name, coords):
        """Internal helper to find the vertical baseline (height/offset) of digits in a box."""
        x, y, w, h = self._get_raw_coords(name, coords)
        
        # Apply mod adjustments if needed
        if self.anne_hk_active and name.endswith("_vils"):
            y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
            y, h = max(0, y - y_adj), h + y_adj
            
        box_img = img[y:y+h, x:x+w].copy()
        ignore_color = (self.anne_hk_active and name.endswith("_vils"))
        out_img = cleanup_box(box_img, ignore_color=ignore_color)
        _, boxes = segment_box(box_img, out_img, self.ui_scale, bright_threshold=self.bright_thresh, ignore_color=ignore_color)
        
        if boxes:
            y_min = min(b['y'] for b in boxes)
            y_max = max(b['y'] + b['h'] for b in boxes)
            return (y_max - y_min), y_min
        return None, None

    def _get_raw_coords(self, name, coords):
        """Scales coordinates from the UI map according to the detected UI scale."""
        return (int(coords['x_px'] * self.ui_scale), int(coords['y_px'] * self.ui_scale),
                int(coords['w_px'] * self.ui_scale), int(coords['h_px'] * self.ui_scale))

    def get_element_coords(self, name, coords):
        """
        Calculates final coordinates for a UI element, applying mod-specific 
        adjustments and vertical clipping where applicable.
        """
        x, y, w, h = self._get_raw_coords(name, coords)
        
        # 1. Mod Adjustment: Anne_HK vils boxes are larger and shifted
        if self.anne_hk_active and name.endswith("_vils"):
            y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
            y, h = max(0, y - y_adj), h + y_adj
            if name == "idle_vils":
                w += int(w * ANNE_HK_IDLE_W_ADJUST_FACTOR)
        
        # 2. Vertical Anchoring: Use detected baselines for all boxes except idle_vils
        clipped = False
        target_ref = None
        
        if name == "idle_vils":
            pass # Exception: Idle indicator is dynamic and requires flexible vertical matching
        elif name.endswith("_vils"):
            target_ref = (self.vil_ref_y, self.vil_ref_h)
        else:
            target_ref = (self.ref_y, self.ref_h)

        if target_ref and target_ref[1] is not None:
            # Add 1px padding above and below to prevent accidental clipping
            y, h, clipped = y + target_ref[0] - 1, target_ref[1] + 2, True
            
        return x, y, w, h, clipped

    def process_box(self, img, name, coords):
        """
        Full processing pipeline for a single UI box: extraction, cleanup, 
        state detection (housed/idle), and segmentation into digits.
        """
        x, y, w, h, clipped = self.get_element_coords(name, coords)
        box_img = img[y:y+h, x:x+w].copy()
        
        overlay_mode, allow_yellow, ignore_color = False, False, False
        if self.anne_hk_active and name.endswith("_vils"):
            ignore_color = True
            
        if name == "population_total":
            overlay_mode, allow_yellow = detect_housed_overlay(box_img), True
            if debug_msg := ("overlay" if overlay_mode else ("yellow text" if contains_yellow(box_img) else None)):
                print(f"  Housed state ({debug_msg}) detected for {name}!")

        out_img = cleanup_box(box_img, overlay_mode, allow_yellow, ignore_color)
        
        # Shortcuts and segmentation
        if name == "idle_vils":
            is_active = contains_red(box_img) if self.anne_hk_active else contains_yellow(box_img)
            if not is_active:
                print("  Idle Villager shortcut: Not active, returning 0 segments.")
                return box_img, [], []
                
        digits, digit_boxes = segment_box(box_img, out_img, self.ui_scale, overlay_mode, 
                                          self.bright_thresh, allow_yellow, ignore_color, clipped)
        return box_img, digits, digit_boxes

def run_pipeline(image_path, ui_map, expected_values=None, debug=False, bright_threshold=230):
    """Main orchestrator for the pipeline."""
    start_time = time.time()
    img = cv2.imread(image_path)
    if img is None: return

    image_name = Path(image_path).stem
    output_base = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'output', 'extractions', image_name)
    shutil.rmtree(output_base, ignore_errors=True)
    os.makedirs(os.path.join(output_base, "digits"), exist_ok=True)
    if debug: os.makedirs(os.path.join(output_base, "boxes"), exist_ok=True)

    pipeline = ExtractorPipeline(ui_map, bright_threshold)
    pipeline.calibrate(img)
    
    core_logic_time = 0
    for name, coords in ui_map.get('elements', {}).items():
        print(f"Processing box: {name}...")
        proc_start = time.time()
        
        box_img, digits, _ = pipeline.process_box(img, name, coords)
        core_logic_time += time.time() - proc_start

        if debug: cv2.imwrite(os.path.join(output_base, "boxes", f"{name}.png"), box_img)
        
        # Determine expected characters for file naming
        res_type, sub_type = name.split('_')[0], (name.split('_')[1] if '_' in name else 'value')
        target_str = expected_values.get(res_type, {}).get(sub_type, "") if expected_values else ""
        
        for i, digit_img in enumerate(digits):
            char = target_str[i] if i < len(target_str) else "extra"
            char_name = "slash" if char == "/" else ("dot" if char == "." else char)
            cv2.imwrite(os.path.join(output_base, "digits", f"{name}_pos_{i:02d}_val_{char_name}.png"), digit_img)

    if debug:
        debug_img = img.copy()
        for name, coords in ui_map.get('elements', {}).items():
            x, y, w, h, _ = pipeline.get_element_coords(name, coords)
            cv2.rectangle(debug_img, (x-1, y-1), (x+w, y+h), (0, 255, 0), 1)
            cv2.putText(debug_img, name, (x, y-6), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 1)
        cv2.imwrite(os.path.join(output_base, f"{image_name}_debug.png"), debug_img)

    print(f"\nPipeline complete in {time.time() - start_time:.4f}s (Core: {core_logic_time:.4f}s)")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("image_path")
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--bright-thresh", type=int, default=230)
    args = parser.parse_args()
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(script_dir, '..', 'ui_map.json'), 'r') as f:
        ui_map = json.load(f)
    
    expected_values = {}
    expected_path = os.path.join(script_dir, '..', 'test_bench', 'expected_values.json')
    if os.path.exists(expected_path):
        with open(expected_path, 'r') as f:
            data = json.load(f)
            img_filename = Path(args.image_path).name
            if img_filename in data.get('images', {}):
                val_set_name = data['images'][img_filename].get('use_value_set')
                expected_values = data.get('value_sets', {}).get(val_set_name, {})
        
    run_pipeline(args.image_path, ui_map, expected_values, args.debug, args.bright_thresh)

if __name__ == "__main__":
    main()
