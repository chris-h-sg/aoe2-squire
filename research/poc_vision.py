import cv2
import numpy as np
import json
import os
import argparse
import time
from pathlib import Path

# ==========================================
# CONSTANTS & CONFIGURATION
# ==========================================

# --- Scale Detection ---
RED_MASK_LOWER = np.array([0, 0, 201])
RED_MASK_UPPER = np.array([60, 60, 255])
BASELINE_MARGIN = 263

# --- Extraction Clean-up ---
OUT_GREY_TOLERANCE = 20
OUT_BRIGHTNESS_THRESHOLD = 5

# --- Segmentation ---
SEG_GREY_TOLERANCE = 20
SEG_BRIGHTNESS_THRESHOLD = 100
BASELINE_MIN_AREA = 15

# --- Matching ---
WORKING_HEIGHT = 36
CANVAS_SIZE = 64
BLUR_SIGMA = 1.0
OFFSETS = [-1, 0, 1]

# Pre-computed translation matrices for Wiggle SSD
WIGGLE_MATRICES = [
    np.float32([[1, 0, dx], [0, 1, dy]])
    for dy in OFFSETS
    for dx in OFFSETS
]

# ==========================================
# 1. UI SCALE DETECTION
# ==========================================

def detect_ui_scale(img, baseline_margin=BASELINE_MARGIN):
    """
    Detects the UI scale based on the right-side margin of red UI elements.
    """
    if img is None:
        return 1.0
        
    h, w, _ = img.shape
    top_h = int(h * 0.2)
    crop = img[0:top_h, :]
    
    # Mask for red pixels
    mask = cv2.inRange(crop, RED_MASK_LOWER, RED_MASK_UPPER)
    y_idxs, x_idxs = np.nonzero(mask)
    
    if len(x_idxs) < 5: 
        return 1.0
        
    # Distance from right edge to the rightmost red pixel
    margin_px = w - np.max(x_idxs)
    return margin_px / baseline_margin

# ==========================================
# 2. EXTRACTION & SEGMENTATION
# ==========================================

def apply_base_filter(img, grey_tol, brightness_thresh):
    """
    Filter by greyness and brightness to isolate white text.
    """
    filtered = img.copy()
    if len(filtered.shape) == 3:
        # Check for color deviation (non-grey pixels)
        max_val = np.max(filtered, axis=2).astype(np.int16)
        min_val = np.min(filtered, axis=2).astype(np.int16)
        too_colored_mask = (max_val - min_val) > grey_tol
        filtered[too_colored_mask] = 0
        filtered = cv2.cvtColor(filtered, cv2.COLOR_BGR2GRAY)
        
    _, cleaned = cv2.threshold(filtered, brightness_thresh, 255, cv2.THRESH_TOZERO)
    return cleaned

def step2_cleanup_box(box_img):
    """
    Produces a high-quality soft-filtered output image for the matcher.
    """
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, OUT_BRIGHTNESS_THRESHOLD)

def step3_segment_into_digits(box_img, out_img, ui_scale=1.0):
    """
    Finds digit bounding boxes using strict iterative filtering on the original BGR,
    then crops the final digits from the cleaner 'out_img'.
    """
    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    
    def get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold, offset_x=0, offset_y=0, connectivity=8):
        # Step 1: Filter and find connected components
        seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold)
        binary = (seg > 0).astype(np.uint8)
        num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
        
        # Filter components by minimum area
        valid_indices = [i for i in range(1, num_labels) if stats[i, cv2.CC_STAT_AREA] >= min_area]
        if not valid_indices:
            return []

        img_h, img_w = roi_bgr.shape[:2]

        # Case A: Stall (1 wide blob, same size as parent)
        if len(valid_indices) == 1:
            idx = valid_indices[0]
            w, h = stats[idx, cv2.CC_STAT_WIDTH], stats[idx, cv2.CC_STAT_HEIGHT]
            
            if w == img_w and h == img_h and (w + 2 > h):
                if grey_tolerance > 2:
                    return get_components_recursive(roi_bgr, grey_tolerance - 2, brightness_threshold, offset_x, offset_y, connectivity)
                if brightness_threshold < 160:
                    return get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold + 10, offset_x, offset_y, connectivity)
                if connectivity == 8:
                    return get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold, offset_x, offset_y, 4)
                return [{'x': offset_x, 'y': offset_y, 'w': w, 'h': h}]

        # Case B: Standard Processing
        results = []
        for i in valid_indices:
            x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
            
            if w + 2 > h:
                # Too wide, mask neighbor pixels and recurse
                isolated_roi = roi_bgr[y:y+h, x:x+w].copy()
                isolated_roi[labels[y:y+h, x:x+w] != i] = 0
                results.extend(get_components_recursive(isolated_roi, grey_tolerance, brightness_threshold, offset_x + x, offset_y + y, connectivity))
            else:
                results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
                
        return results

    # 1. Find the bounding boxes
    digit_boxes = get_components_recursive(box_img, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD)
    digit_boxes.sort(key=lambda d: d['x'])
    
    # 2. Crop from the clean image
    digits = []
    for db in digit_boxes:
        crop = out_img[db['y'] : db['y']+db['h'], db['x'] : db['x']+db['w']]
        digits.append(crop)
        
    return digits, digit_boxes

def extract_digits(box_img, ui_scale=1.0):
    """
    Wrapper that performs cleanup and segmentation.
    Returns list of cleaned digit images.
    """
    out_img = step2_cleanup_box(box_img)
    digits, _ = step3_segment_into_digits(box_img, out_img, ui_scale)
    return digits

# ==========================================
# 3. MATCHING LOGIC
# ==========================================

def calculate_h_symmetry(canvas):
    """Calculates horizontal symmetry score (lower is more symmetric)."""
    flipped = cv2.flip(canvas, 1) # 1 = horizontal flip
    diff = canvas - flipped
    return np.sum(diff * diff)

def prepare_canvas(img, target_h):
    """Scales, centers by bounding box, and blurs an image into a fixed canvas."""
    h_orig, w_orig = img.shape
    if h_orig == 0 or w_orig == 0:
        return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)

    # 1. Upscale
    scale = target_h / h_orig
    new_w = max(1, int(w_orig * scale))
    upscaled = cv2.resize(img, (new_w, target_h), interpolation=cv2.INTER_CUBIC if scale > 1 else cv2.INTER_AREA)
    
    # 2. Find bounding box of the actual content
    _, thresh = cv2.threshold(upscaled, 1, 255, cv2.THRESH_BINARY)
    coords = cv2.findNonZero(thresh)
    if coords is None:
        return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)
    
    x, y, w, h = cv2.boundingRect(coords)
    crop = upscaled[y:y+h, x:x+w]
    
    # 3. Center in canvas based on GEOMETRIC center of bounding box
    canvas = np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.uint8)
    off_x = (CANVAS_SIZE - w) // 2
    off_y = (CANVAS_SIZE - h) // 2
    
    # Safety check for bounds
    if h > CANVAS_SIZE or w > CANVAS_SIZE:
        # Resize again if it still doesn't fit (rare edge case)
        scale_fit = min(CANVAS_SIZE/h, CANVAS_SIZE/w)
        crop = cv2.resize(crop, None, fx=scale_fit, fy=scale_fit, interpolation=cv2.INTER_AREA)
        h, w = crop.shape
        off_x = (CANVAS_SIZE - w) // 2
        off_y = (CANVAS_SIZE - h) // 2
        
    canvas[off_y:off_y+h, off_x:off_x+w] = crop
    
    # 4. Blur and convert to float (0.0 to 1.0)
    canvas_f = canvas.astype(np.float32) / 255.0
    blurred = cv2.GaussianBlur(canvas_f, (0, 0), BLUR_SIGMA)
    return blurred

def load_templates(templates_dir):
    """Loads and pre-processes digit templates."""
    templates = {}
    if not os.path.exists(templates_dir):
        print(f"Template directory {templates_dir} not found!")
        return templates

    for filename in os.listdir(templates_dir):
        if filename.endswith(".png"):
            char = filename.replace(".png", "")
            if char == "slash":
                char = "/"
            img = cv2.imread(os.path.join(templates_dir, filename), cv2.IMREAD_GRAYSCALE)
            if img is not None:
                # Pre-process template into canvas now to save time later
                templates[char] = prepare_canvas(img, WORKING_HEIGHT)
    return templates

def match_digit_to_template(digit_img, processed_templates):
    """
    Matches a digit image using 'Wiggle SSD' and 'Symmetry Tie-Breaker'.
    OPTIMIZATION: Pre-warps the input digit (9 shifts) instead of every template (11*9 shifts).
    Returns (best_char, confidence, debug_info).
    """
    input_canvas = prepare_canvas(digit_img, WORKING_HEIGHT)
    
    # 1. Pre-compute 9 shifted versions of the input canvas
    # We invert the offsets because shifting Input LEFT is equivalent to shifting Template RIGHT.
    shifted_inputs = []
    # Prioritize (0,0) by putting it first might be nice, but we need all 9 anyway for best_ssd
    # OFFSETS = [-1, 0, 1]
    
    for dy in OFFSETS:
        for dx in OFFSETS:
            # Shift Input by (-dx, -dy)
            M = np.float32([[1, 0, -dx], [0, 1, -dy]])
            shifted = cv2.warpAffine(input_canvas, M, (CANVAS_SIZE, CANVAS_SIZE))
            shifted_inputs.append(shifted)

    all_matches = []
    
    # 2. Compare against all templates
    for char, template_canvas in processed_templates.items():
        best_ssd = float('inf')
        
        # Check against all 9 pre-shifted inputs
        for shifted_input in shifted_inputs:
            diff = shifted_input - template_canvas
            ssd = np.sum(diff * diff)
            if ssd < best_ssd:
                best_ssd = ssd
        
        all_matches.append((best_ssd, char))
    
    all_matches.sort()
    
    if not all_matches:
        return "?", 0.0, "No Templates"

    # Tie-Breaker Logic
    tb_info = ""
    if len(all_matches) > 1:
        best_ssd, best_char = all_matches[0]
        second_ssd, second_char = all_matches[1]
        
        asym_chars = {'3', '6', '9'}
        is_conflict = (best_char == '0' and second_char in asym_chars) or \
                      (best_char in asym_chars and second_char == '0')
        
        # Trigger if SSD difference is less than 20% of the winner
        if is_conflict and (second_ssd - best_ssd < best_ssd * 0.20):
            sym_score = calculate_h_symmetry(input_canvas)
            
            # 1. Identified as 0 but is asymmetric (SymH > 60) -> Swap to 3/6/9
            if best_char == '0' and sym_score > 60:
                all_matches[0], all_matches[1] = all_matches[1], all_matches[0]
                tb_info = f"[SwapH:{sym_score:.1f}]"
            # 2. Identified as 3/6/9 but is symmetric (SymH < 40) -> Swap to 0
            elif best_char in asym_chars and sym_score < 40:
                all_matches[0], all_matches[1] = all_matches[1], all_matches[0]
                tb_info = f"[SwapH:{sym_score:.1f}]"
            else:
                tb_info = f"[TrustH:{sym_score:.1f}]"
    
    final_best_ssd, final_best_char = all_matches[0]
    return final_best_char, final_best_ssd, tb_info

# ==========================================
# 4. MAIN PIPELINE
# ==========================================

def process_image(image_path, ui_map, templates):
    start_time = time.time()
    
    img = cv2.imread(image_path)
    if img is None:
        print(f"Failed to load {image_path}")
        return

    # Start measuring processing time (after disk I/O)
    proc_start = time.time()

    # 1. Detect Scale
    t0 = time.time()
    ui_scale = detect_ui_scale(img)
    time_scale = time.time() - t0
    print(f"Detected UI Scale: {ui_scale:.4f} (took {time_scale*1000:.2f}ms)")

    results = {}
    time_extract = 0
    time_match = 0

    # 2. Process each element
    for name, coords in ui_map.get('elements', {}).items():
        x = int(coords['x_px'] * ui_scale)
        y = int(coords['y_px'] * ui_scale)
        w = int(coords['w_px'] * ui_scale)
        h = int(coords['h_px'] * ui_scale)
        
        # Valid crop check
        if x+w > img.shape[1] or y+h > img.shape[0]:
            print(f"Skipping {name}: Coordinates out of bounds")
            continue
            
        box_img = img[y:y+h, x:x+w].copy()
        
        # Extract digits
        t0 = time.time()
        digits = extract_digits(box_img, ui_scale)
        time_extract += (time.time() - t0)
        
        # Match digits
        t0 = time.time()
        value_str = ""
        debug_details = []
        for d_img in digits:
            char, ssd, info = match_digit_to_template(d_img, templates)
            value_str += char
            debug_details.append(f"{char}({ssd:.1f}{info})")
        time_match += (time.time() - t0)
            
        print(f"  {name:<20}: {value_str:<10} | Raw: {', '.join(debug_details)}")
        
        # Structure output to match expected_values.json (category -> sub_key)
        parts = name.split('_')
        category = parts[0]
        sub_key = parts[1] if len(parts) > 1 else "value"
        
        if category not in results:
            results[category] = {}
        results[category][sub_key] = value_str

    proc_time = time.time() - proc_start
    total_time = time.time() - start_time
    
    print(f"\nTiming:")
    print(f"  Scale Detection: {time_scale*1000:.2f}ms")
    print(f"  Extraction:      {time_extract*1000:.2f}ms")
    print(f"  Matching:        {time_match*1000:.2f}ms")
    print(f"  ---------------------------")
    print(f"  Core Logic Total:{proc_time*1000:.2f}ms")
    print(f"  Total Time:      {total_time*1000:.2f}ms")
    
    return results

def main():
    parser = argparse.ArgumentParser(description="POC Vision: Extract and Match Digits")
    parser.add_argument("--image", type=str, help="Path to input image")
    args = parser.parse_args()

    # Determine paths
    script_dir = os.path.dirname(os.path.abspath(__file__))
    
    # 1. Load UI Map
    map_path = os.path.join(script_dir, '..', 'ui_map.json')
    if not os.path.exists(map_path):
        print(f"Error: {map_path} not found.")
        return
    with open(map_path, 'r') as f:
        ui_map = json.load(f)

    # 2. Load Templates
    templates_dir = os.path.join(script_dir, 'templates', 'enormous_numbers')
    templates = load_templates(templates_dir)
    print(f"Loaded {len(templates)} templates from {templates_dir}")

    # 3. Select Image
    if args.image:
        image_path = args.image
    else:
        # Default test image
        image_path = os.path.join(script_dir, '..', 'test_bench', 'aoe2_16x9.png')
    
    if not os.path.exists(image_path):
        print(f"Error: Image {image_path} not found.")
        return

    # 4. Run Pipeline
    process_image(image_path, ui_map, templates)

if __name__ == "__main__":
    main()
