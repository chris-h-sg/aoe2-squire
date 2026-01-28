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

def apply_base_filter(img, grey_tol, brightness_thresh):
    """Filter by greyness and brightness."""
    filtered = img.copy()
    if len(filtered.shape) == 3:
        max_val = np.max(filtered, axis=2).astype(np.int16)
        min_val = np.min(filtered, axis=2).astype(np.int16)
        too_colored_mask = (max_val - min_val) > grey_tol
        filtered[too_colored_mask] = 0
        filtered = cv2.cvtColor(filtered, cv2.COLOR_BGR2GRAY)
    _, cleaned = cv2.threshold(filtered, brightness_thresh, 255, cv2.THRESH_TOZERO)
    return cleaned

def step2_cleanup_box(box_img):
    """
    Step 2: Cleanup.
    - Implements static 'soft' filtering for final output.
    """
    # --- ADJUSTABLE THRESHOLDS ---
    OUT_GREY_TOLERANCE = 12
    OUT_BRIGHTNESS_THRESHOLD = 5
    # ----------------------------
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, OUT_BRIGHTNESS_THRESHOLD)

def step3_segment_into_digits(box_img, out_img, ui_scale=1.0):
    """
    Step 3: Segmentation into digits.
    - Uses iterative strict filtering on original BGR image to find 'seeds'.
    - Crops final digits from pre-cleaned 'out_img'.
    """
    # --- ADJUSTABLE THRESHOLDS ---
    SEG_GREY_TOLERANCE = 10
    SEG_BRIGHTNESS_THRESHOLD = 100
    BASELINE_MIN_AREA = 15
    # ----------------------------

    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    
    def get_components_recursive(roi_bgr, grey_tol, brightness_thresh, offset_x=0, offset_y=0, connectivity=8):
        seg = apply_base_filter(roi_bgr, grey_tol, brightness_thresh)
        _, binary = cv2.threshold(seg, 1, 255, cv2.THRESH_BINARY)
        num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
        
        valid_indices = [i for i in range(1, num_labels) if stats[i, cv2.CC_STAT_AREA] >= min_area]
        
        if not valid_indices: return []
        
        if len(valid_indices) > 1:
            res = []
            for i in valid_indices:
                x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
                res.extend(get_components_recursive(roi_bgr[y:y+h, x:x+w], grey_tol, brightness_thresh, offset_x + x, offset_y + y, connectivity))
            return res
            
        i = valid_indices[0]
        x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
        
        if w + 2 > h:
            if grey_tol > 2:
                return get_components_recursive(roi_bgr[y:y+h, x:x+w], grey_tol - 2, brightness_thresh, offset_x + x, offset_y + y, connectivity)
            if brightness_thresh < 160:
                return get_components_recursive(roi_bgr[y:y+h, x:x+w], grey_tol, brightness_thresh + 10, offset_x + x, offset_y + y, connectivity)
            if connectivity == 8:
                return get_components_recursive(roi_bgr[y:y+h, x:x+w], grey_tol, brightness_thresh, offset_x + x, offset_y + y, 4)
            print(f"  Exiting with {w}x{h}")
        
        # Base case: Final segment found
        return [{'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h}]

    # 1. Find the bounding boxes using iterative strict filter on original BGR
    digit_boxes = get_components_recursive(box_img, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD)
    digit_boxes.sort(key=lambda d: d['x'])
    
    digits = []
    for db in digit_boxes:
        crop = out_img[db['y'] : db['y']+db['h'], db['x'] : db['x']+db['w']]
        digits.append(crop)
        
    print(f"  Segmentation iterative: found {len(digits)} components.")
    return digits

def run_pipeline(image_path, ui_map):
    start_time = time.time()
    
    img = cv2.imread(image_path)
    if img is None: return
    
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
        box_img = img[y:y+h, x:x+w].copy()
        
        print(f"Processing box: {name}...")
        
        # Step 2: Cleanup (Produces high-quality soft-filtered output image)
        out_img = step2_cleanup_box(box_img)
        
        # Step 3: Segmentation (Uses original BGR for seeds, then crops from out_img)
        digits = step3_segment_into_digits(box_img, out_img, ui_scale)
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

    total_time = time.time() - start_time
    print(f"\nPipeline complete in {total_time:.4f}s")
    print(f"Core logic took {core_logic_time:.4f}s (excludes image read/write)")
    print(f"Digits saved in {digit_dir}")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("image_path")
    args = parser.parse_args()
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(script_dir, '..', 'ui_map.json'), 'r') as f:
        ui_map = json.load(f)
        
    run_pipeline(args.image_path, ui_map)

if __name__ == "__main__":
    main()
