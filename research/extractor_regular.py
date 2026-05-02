"""
Extractor Pipeline - Structural processing of regular (scaled) UI images
"""

import cv2
import numpy as np
import json
import os
import argparse
import shutil
import time
from pathlib import Path

# Red pixel detection for scaling
RED_MASK_LOWER = np.array([0, 0, 201])
RED_MASK_UPPER = np.array([60, 60, 255])

def detect_ui_scale(img, baseline_margin=263):
    h, w, _ = img.shape
    top_h = int(h * 0.2)
    crop = img[0:top_h, :]
    mask = cv2.inRange(crop, RED_MASK_LOWER, RED_MASK_UPPER)
    y_idxs, x_idxs = np.nonzero(mask)
    if len(x_idxs) < 5: return 1.0
    return (w - np.max(x_idxs)) / baseline_margin

def apply_base_filter(img, grey_tol, brightness_thresh, overlay_mode=False, allow_yellow=False):
    """Filter by greyness and brightness. If overlay_mode, use background color subtraction."""
    if overlay_mode:
        # Detect overlay color from a near-black background pixel (top-left)
        bg_color = img[2, 2].astype(np.int16)
        
        # Undo the overlay by subtracting the background color
        subtracted = np.clip(img.astype(np.int16) - bg_color, 0, 255).astype(np.uint8)
        
        # Convert to grayscale to avoid grey_tol issues with the remaining tint
        gray_sub = cv2.cvtColor(subtracted, cv2.COLOR_BGR2GRAY)
        
        # Normalize to bring the text to full brightness
        normalized = cv2.normalize(gray_sub, None, 0, 255, cv2.NORM_MINMAX)
        
        _, cleaned = cv2.threshold(normalized, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if len(img.shape) == 3:
        # BGR Channels
        b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
        max_val = np.max(img, axis=2).astype(np.int16)
        min_val = np.min(img, axis=2).astype(np.int16)
        diff = max_val - min_val
        
        # 1. Standard grey mask (keeps white/grey text)
        to_keep = (diff <= grey_tol)
        
        # 2. Dynamic Yellow Font Detection (Restricted to allow_yellow fields):
        if allow_yellow:
            rg_diff = np.abs(r.astype(np.int16) - g.astype(np.int16))
            is_yellow = (r > 150) & (g > 150) & (rg_diff < 50) & (b < max_val - 15)
            if np.any(is_yellow):
                to_keep |= is_yellow
        
        # Use max_val for grayscale to ensure colored text matches white templates
        filtered = np.zeros(max_val.shape, dtype=np.uint8)
        filtered[to_keep] = max_val.astype(np.uint8)[to_keep]
    else:
        filtered = img

    _, cleaned = cv2.threshold(filtered, brightness_thresh, 255, cv2.THRESH_TOZERO)
    return cleaned

def detect_housed_overlay(img):
    """Detects the bright yellow background overlay used when a player is housed."""
    if img is None or len(img.shape) < 3:
        return False
    # The overlay background is very bright in Green and Red (mean > 150)
    # and significantly darker in Blue (mean < 100).
    mean_g = np.mean(img[:, :, 1])
    mean_r = np.mean(img[:, :, 2])
    mean_b = np.mean(img[:, :, 0])
    return mean_g > 150 and mean_r > 150 and mean_b < 100

def contains_yellow(img, min_brightness=100, blue_margin=50, rg_similarity=50):
    """
    Checks if the box contains yellow pixels (indicating active idle villagers icon).
    Yellow is detected by: high R and G values, low B value, R and G similar.
    """
    if img is None or len(img.shape) < 3:
        return False
    
    # Extract BGR channels
    b_channel = img[:, :, 0]
    g_channel = img[:, :, 1]
    r_channel = img[:, :, 2]
    
    # Yellow detection criteria
    # 1. G and R must be bright
    bright_g = g_channel > min_brightness
    bright_r = r_channel > min_brightness
    
    # 2. B must be significantly lower than both G and R
    b_lower_than_g = b_channel < (g_channel - blue_margin)
    b_lower_than_r = b_channel < (r_channel - blue_margin)
    
    # 3. R and G should be similar (both high for yellow)
    rg_similar = np.abs(r_channel.astype(np.int16) - g_channel.astype(np.int16)) < rg_similarity
    
    # Combine all criteria
    is_yellow = bright_g & bright_r & b_lower_than_g & b_lower_than_r & rg_similar
    
    # Return True if any yellow pixels found
    return np.any(is_yellow)

def step2_cleanup_box(box_img, overlay_mode=False, allow_yellow=False):
    """
    Step 2: Cleanup.
    - Implements static 'soft' filtering for final output.
    """
    # --- ADJUSTABLE THRESHOLDS ---
    OUT_GREY_TOLERANCE = 20
    OUT_BRIGHTNESS_THRESHOLD = 5 if not overlay_mode else 30 # Higher threshold for normalized Blue
    # ----------------------------
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, OUT_BRIGHTNESS_THRESHOLD, overlay_mode=overlay_mode, allow_yellow=allow_yellow)

def step3_segment_into_digits(box_img, out_img, ui_scale=1.0, overlay_mode=False, bright_threshold=230, allow_yellow=False):
    """
    Step 3: Segmentation into digits.
    - Uses iterative strict filtering on original BGR image to find 'seeds'.
    - Crops final digits from pre-cleaned 'out_img'.
    """
    # --- ADJUSTABLE THRESHOLDS ---
    SEG_GREY_TOLERANCE = 30
    SEG_BRIGHTNESS_THRESHOLD = 100
    SEG_REQUIRED_BRIGHTNESS = bright_threshold
    BASELINE_MIN_AREA = 15
    # ----------------------------

    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    
    def get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold, offset_x=0, offset_y=0, connectivity=8):
        # Step 1: Filter and find connected components
        seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold, overlay_mode=overlay_mode, allow_yellow=allow_yellow)
        binary = (seg > 0).astype(np.uint8)
        num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
        
        # Filter components by minimum area and brightness
        valid_indices = []
        for i in range(1, num_labels):
            if stats[i, cv2.CC_STAT_AREA] >= min_area:
                # Check if component has at least one bright pixel
                x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
                if np.max(seg[y:y+h, x:x+w][labels[y:y+h, x:x+w] == i]) >= SEG_REQUIRED_BRIGHTNESS:
                    valid_indices.append(i)

        if not valid_indices:
            return []

        img_h, img_w = roi_bgr.shape[:2]

        # Case A: Stall (1 wide blob, same size as parent)
        # If we have 1 component that occupies the full dimensions and is still wide, force threshold refinement.
        if len(valid_indices) == 1:
            idx = valid_indices[0]
            w = stats[idx, cv2.CC_STAT_WIDTH]
            h = stats[idx, cv2.CC_STAT_HEIGHT]
            
            if w == img_w and h == img_h and (w + 2 > h):
                if brightness_threshold < 160:
                    return get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold + 10, offset_x, offset_y, connectivity)
                if grey_tolerance > 2:
                    return get_components_recursive(roi_bgr, grey_tolerance - 2, brightness_threshold, offset_x, offset_y, connectivity)
                if connectivity == 8:
                    return get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold, offset_x, offset_y, 4)
                
                # Base Case: Exhausted refinements, return current box
                return [{'x': offset_x, 'y': offset_y, 'w': w, 'h': h}]

        # Case B: Standard Processing
        # Iterate through found components. If too wide, mask and recurse for a split.
        results = []
        for i in valid_indices:
            x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
            
            if w + 2 > h:
                # ISOLATION MASKING: Create a crop and zero out detached pixels
                isolated_roi = roi_bgr[y:y+h, x:x+w].copy()
                isolated_roi[labels[y:y+h, x:x+w] != i] = 0
                results.extend(get_components_recursive(isolated_roi, grey_tolerance, brightness_threshold, offset_x + x, offset_y + y, connectivity))
            else:
                # Base Case: Clean digit or good shape found
                results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
                
        return results

    # 1. Find the bounding boxes using iterative strict filter on original BGR
    digit_boxes = get_components_recursive(box_img, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD)
    digit_boxes.sort(key=lambda d: d['x'])
    
    digits = []
    for db in digit_boxes:
        crop = out_img[db['y'] : db['y']+db['h'], db['x'] : db['x']+db['w']]
        digits.append(crop)
        
    print(f"  Segmentation iterative: found {len(digits)} components.")
    return digits

def run_pipeline(image_path, ui_map, debug=False, bright_threshold=230):
    start_time = time.time()
    
    img = cv2.imread(image_path)
    if img is None: return
    
    debug_img = img.copy() if debug else None
    
    image_name = Path(image_path).stem
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_base = os.path.join(script_dir, 'output', 'extractions', image_name)
    shutil.rmtree(output_base, ignore_errors=True)
    os.makedirs(output_base, exist_ok=True)
    
    loop_start = time.time()
    core_logic_time = 0
    
    # Core Logic: Scale Detection
    scale_start = time.time()
    ui_scale = detect_ui_scale(img, ui_map.get('baseline_margin', 263))
    core_logic_time += time.time() - scale_start
    print(f"UI Scale detected: {ui_scale:.4f} (took {time.time() - scale_start:.4f}s)")
    
    # Box extraction from UI Map
    digit_dir = os.path.join(output_base, "digits")
    os.makedirs(digit_dir, exist_ok=True)
    
    box_dir = os.path.join(output_base, "boxes") if debug else None
    if box_dir:
        os.makedirs(box_dir, exist_ok=True)
    
    # Load Expected Values
    expected_values = {}
    expected_path = os.path.join(script_dir, '..', 'test_bench', 'expected_values.json')
    if os.path.exists(expected_path):
        with open(expected_path, 'r') as f:
            data = json.load(f)
            img_filename = f"{image_name}.png"
            if img_filename in data.get('images', {}):
                val_set_name = data['images'][img_filename].get('use_value_set')
                expected_values = data.get('value_sets', {}).get(val_set_name, {})

    for name, coords in ui_map.get('elements', {}).items():        
        # Core Logic: Pre-processing and Segmentation
        proc_start = time.time()
        x = int(coords['x_px'] * ui_scale)
        y = int(coords['y_px'] * ui_scale)
        w = int(coords['w_px'] * ui_scale)
        h = int(coords['h_px'] * ui_scale)

        if debug_img is not None:
            # Draw border 1px outside the box with 1px thickness
            cv2.rectangle(debug_img, (x-1, y-1), (x+w, y+h), (0, 255, 0), 1)
            cv2.putText(debug_img, name, (x, y-6), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 1)

        box_img = img[y:y+h, x:x+w].copy()
        
        if box_dir:
            cv2.imwrite(os.path.join(box_dir, f"{name}.png"), box_img)

        print(f"Processing box: {name}...")
        
        # Select processing modes
        overlay_mode = False
        allow_yellow = False

        # Population Total Shortcut: Check for Housed overlay and enable Yellow Font detection
        if name == "population_total":
            overlay_mode = detect_housed_overlay(box_img)
            allow_yellow = True
            
            if overlay_mode:
                pop_color = "overlay"
            elif contains_yellow(box_img):
                pop_color = "yellow"
            else:
                pop_color = "white"
                
            if pop_color != "white":
                state_desc = "overlay" if pop_color == "overlay" else "yellow text"
                print(f"  Housed state ({state_desc}) detected for {name}!")

        # Step 2: Cleanup (Produces high-quality soft-filtered output image)
        out_img = step2_cleanup_box(box_img, overlay_mode=overlay_mode, allow_yellow=allow_yellow)
        
        # Idle Villager Shortcut: If no yellow pixels, return no digits
        if name == "idle_vils" and not contains_yellow(box_img):
            print("  Idle Villager shortcut: No yellow found, returning 0 segments.")
            digits = []
        else:
            # Step 3: Segmentation (Uses original BGR for seeds, then crops from out_img)
            digits = step3_segment_into_digits(box_img, out_img, ui_scale, overlay_mode=overlay_mode, bright_threshold=bright_threshold, allow_yellow=allow_yellow)
        
        core_logic_time += time.time() - proc_start
        
        resource_type = name.split('_')[0]
        sub_type = name.split('_')[1] if '_' in name else 'value'
        target_str = expected_values.get(resource_type, {}).get(sub_type, "")
        
        # Disk I/O: Image writing
        for i, digit_img in enumerate(digits):
            char = target_str[i] if i < len(target_str) else "extra"
            char_name = "slash" if char == "/" else ("dot" if char == "." else char)
            filename = f"{name}_pos_{i:02d}_val_{char_name}.png"
            cv2.imwrite(os.path.join(digit_dir, filename), digit_img)

    if debug_img is not None:
        debug_out_path = os.path.join(output_base, f"{image_name}_debug.png")
        cv2.imwrite(debug_out_path, debug_img)
        print(f"Debug image saved to: {debug_out_path}")

    total_time = time.time() - start_time
    print(f"\nPipeline complete in {total_time:.4f}s")
    print(f"Core logic took {core_logic_time:.4f}s (excludes image read/write)")
    print(f"Digits saved in {digit_dir}")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("image_path")
    parser.add_argument("--debug", action="store_true", help="Output a debug image with extraction boxes")
    parser.add_argument("--bright-thresh", type=int, default=230, help="Minimum brightness a segment must contain to be kept")
    args = parser.parse_args()
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(script_dir, '..', 'ui_map.json'), 'r') as f:
        ui_map = json.load(f)
        
    run_pipeline(args.image_path, ui_map, debug=args.debug, bright_threshold=args.bright_thresh)

if __name__ == "__main__":
    main()
