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
RED_PIXEL_MIN_COUNT = 12
RED_DIFF_THRESHOLD = 140     # Red must be this much higher than Green and Blue
BASELINE_MARGIN = 263
ANCHOR_MIN_X = 0.5
ANCHOR_MAX_X = 0.95
ANCHOR_MIN_Y = 0.015
ANCHOR_MAX_Y = 0.035
UI_SCALE_MIN = 0.5
UI_SCALE_MAX = 2.0

# --- Mod Support ---
ANNE_HK_PIXEL_MIN_COUNT = 12
ANNE_HK_COLOR_DIFF_THRESHOLD = 150  # R and G must be this much higher than B
ANNE_HK_Y_ADJUST_FACTOR = 0.6       # Villager boxes expanded up by 60%
ANNE_HK_IDLE_W_ADJUST_FACTOR = 0.4   # Idle vils box expanded width by 20%
ANNE_HK_IDLE_RED_MIN = 150          # Red indicator threshold
ANNE_HK_IDLE_GB_MAX = 10            # Max Green/Blue for the red indicator

# --- Extraction Clean-up ---
OUT_GREY_TOLERANCE = 30
OUT_BRIGHTNESS_THRESHOLD = 5

# --- Segmentation ---
SEG_GREY_TOLERANCE = 30
SEG_BRIGHTNESS_THRESHOLD = 100
SEG_REQUIRED_BRIGHTNESS = 230
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
    Ported from src/pipeline/anchor.rs to ensure parity with Rust.
    """
    if img is None:
        return None

    h, w, _ = img.shape
    
    # Calculate scan boundaries
    y_min_scan = int(h * ANCHOR_MIN_Y)
    y_max_scan = min(h - 1, int(h * ANCHOR_MAX_Y))
    x_min_scan = int(w * ANCHOR_MIN_X)
    x_max_scan = min(w - 1, int(w * ANCHOR_MAX_X))
    
    # Convert to RGB for parity with Rust 'image' crate logic
    img_rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
    
    best_rightmost_x = None
    best_y = None

    for y in range(y_min_scan, y_max_scan + 1):
        for x in range(x_min_scan, x_max_scan + 1):
            r, g, b = img_rgb[y, x]
            
            # Red must be at least RED_DIFF_THRESHOLD greater than both Green and Blue
            if (int(r) - int(g) >= RED_DIFF_THRESHOLD) and (int(r) - int(b) >= RED_DIFF_THRESHOLD):
                # Density check: count red pixels in a 10x10 box extending to the left and down
                x_start = max(x_min_scan, x - 9)
                y_end = min(y_max_scan, y + 9)

                # Sub-scan the 10x10 area
                sub_area = img_rgb[y : y_end + 1, x_start : x + 1]
                
                # Vectorized difference check
                r_chan = sub_area[:, :, 0].astype(np.int16)
                g_chan = sub_area[:, :, 1].astype(np.int16)
                b_chan = sub_area[:, :, 2].astype(np.int16)
                r_mask = (r_chan - g_chan >= RED_DIFF_THRESHOLD) & \
                         (r_chan - b_chan >= RED_DIFF_THRESHOLD)
                
                count = np.sum(r_mask)

                if count >= RED_PIXEL_MIN_COUNT:
                    if best_rightmost_x is None or x > best_rightmost_x:
                        best_rightmost_x = x
                        best_y = y

    if best_rightmost_x is None:
        return None
        
    print(f"Anchor found at: x={best_rightmost_x}, y={best_y}")
    margin_px = w - best_rightmost_x
    ui_scale = margin_px / baseline_margin

    if not (UI_SCALE_MIN <= ui_scale <= UI_SCALE_MAX):
        return None

    return ui_scale

# ==========================================
# 2. EXTRACTION & SEGMENTATION
# ==========================================

def apply_base_filter(img, grey_tol, brightness_thresh, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """
    Filter by greyness and brightness to isolate text.
    ignore_color: If True, bypass color-based filtering (greyscale only).
    """
    if img is None or img.size == 0:
        return np.zeros((1, 1), dtype=np.uint8)

    if ignore_color:
        # Convert to greyscale as requested for mod detection
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        _, cleaned = cv2.threshold(gray, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if overlay_mode:
        # Detect overlay color from a near-black background pixel (top-left)
        # Ensure we have enough pixels for [2,2]
        h, w = img.shape[:2]
        if h < 3 or w < 3:
            return np.zeros((h, w), dtype=np.uint8)
            
        bg_color = img[2, 2].astype(np.int16)
        subtracted = np.clip(img.astype(np.int16) - bg_color, 0, 255).astype(np.uint8)
        gray_sub = cv2.cvtColor(subtracted, cv2.COLOR_BGR2GRAY)
        normalized = cv2.normalize(gray_sub, None, 0, 255, cv2.NORM_MINMAX)
        _, cleaned = cv2.threshold(normalized, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if len(img.shape) == 3:
        # BGR Channels
        b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
        max_val = np.max(img, axis=2).astype(np.int16)
        min_val = np.min(img, axis=2).astype(np.int16)
        diff = max_val - min_val
        
        to_keep = (diff <= grey_tol)
        
        if allow_yellow:
            rg_diff = np.abs(r.astype(np.int16) - g.astype(np.int16))
            is_yellow = (r > 150) & (g > 150) & (rg_diff < 50) & (b < max_val - 15)
            if np.any(is_yellow):
                to_keep |= is_yellow
        
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
    
    Args:
        min_brightness: Minimum value for R and G channels (default 100)
        blue_margin: How much lower B must be than R and G (default 50)
        rg_similarity: Maximum difference between R and G (default 50)
    
    Returns:
        True if any yellow pixels found, False otherwise (grey icon = 0 idle vils)
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

def contains_red(img):
    """
    Checks if the box contains red pixels (Anne_HK mod idle villager indicator).
    Red is detected by: high R value, very low G and B values.
    """
    if img is None or len(img.shape) < 3:
        return False
    
    b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    
    is_red = (r > ANNE_HK_IDLE_RED_MIN) & (g < ANNE_HK_IDLE_GB_MAX) & (b < ANNE_HK_IDLE_GB_MAX)
    return np.any(is_red)

def detect_anne_hk_mod(img):
    """
    Detects 'Anne_HK resource panels' mod by checking for specific color patterns in wood_vils box.
    Check if it contains enough pixels where R and G are significantly higher than B.
    """
    if img is None or len(img.shape) < 3:
        return False
    
    # BGR channels
    b = img[:, :, 0].astype(np.int16)
    g = img[:, :, 1].astype(np.int16)
    r = img[:, :, 2].astype(np.int16)
    
    mask = (r - b > ANNE_HK_COLOR_DIFF_THRESHOLD) & (g - b > ANNE_HK_COLOR_DIFF_THRESHOLD)
    count = np.sum(mask)
    
    return count >= ANNE_HK_PIXEL_MIN_COUNT

def step2_cleanup_box(box_img, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """
    Produces a high-quality soft-filtered output image for the matcher.
    """
    # Higher threshold for normalized Blue (30) to avoid background noise
    threshold = OUT_BRIGHTNESS_THRESHOLD if not overlay_mode else 30
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, threshold, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)

def step3_segment_into_digits(box_img, out_img, ui_scale=1.0, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """
    Finds digit bounding boxes using strict iterative filtering on the original BGR,
    then crops the final digits from the cleaner 'out_img'.
    """
    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    
    def get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold, offset_x=0, offset_y=0, connectivity=8):
        # Step 1: Filter and find connected components
        seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
        binary = (seg > 0).astype(np.uint8)
        num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
        
        # Filter components by minimum area and brightness
        valid_indices = []
        for i in range(1, num_labels):
            if stats[i, cv2.CC_STAT_AREA] >= min_area:
                # Check if component has at least one bright pixel
                x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
                req_bright = SEG_REQUIRED_BRIGHTNESS if not ignore_color else 150
                if np.max(seg[y:y+h, x:x+w][labels[y:y+h, x:x+w] == i]) >= req_bright:
                    valid_indices.append(i)

        if not valid_indices:
            return []

        img_h, img_w = roi_bgr.shape[:2]

        # Case A: Stall (1 wide blob, same size as parent)
        if len(valid_indices) == 1:
            idx = valid_indices[0]
            w, h = stats[idx, cv2.CC_STAT_WIDTH], stats[idx, cv2.CC_STAT_HEIGHT]
            
            if w == img_w and h == img_h and (w + 2 > h):
                if brightness_threshold < 160:
                    return get_components_recursive(roi_bgr, grey_tolerance, brightness_threshold + 10, offset_x, offset_y, connectivity)
                if grey_tolerance > 2:
                    return get_components_recursive(roi_bgr, grey_tolerance - 2, brightness_threshold, offset_x, offset_y, connectivity)
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

def extract_digits(box_img, ui_scale=1.0, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """
    Wrapper that performs cleanup and segmentation.
    Returns list of cleaned digit images.
    """
    out_img = step2_cleanup_box(box_img, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
    digits, _ = step3_segment_into_digits(box_img, out_img, ui_scale, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
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
    # Rust uses CatmullRom (Cubic) for upscaling and Triangle (Bilinear) for downscaling
    interp = cv2.INTER_CUBIC if scale > 1 else cv2.INTER_LINEAR
    upscaled = cv2.resize(img, (new_w, target_h), interpolation=interp)
    
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

def process_frame(img, ui_map, templates, verbose=True):
    """
    Process a single frame (numpy array) and extract all UI elements.
    Returns a dictionary with extracted values.
    """
    if img is None:
        return None

    # 1. Detect Scale
    ui_scale = detect_ui_scale(img)
    if ui_scale is None:
        if verbose:
            print("[no anchor] Red UI reference not found — game not visible or UI changed.")
        return {}

    results = {}
    pop_color = "white"
    
    # 2. Mod Detection: Anne_HK resource panels
    # Detect once before processing elements
    anne_hk_active = False
    wood_vils_coords = ui_map.get('elements', {}).get('wood_vils')
    if wood_vils_coords:
        x = int(wood_vils_coords['x_px'] * ui_scale)
        y = int(wood_vils_coords['y_px'] * ui_scale)
        w = int(wood_vils_coords['w_px'] * ui_scale)
        h = int(wood_vils_coords['h_px'] * ui_scale)
        if x+w <= img.shape[1] and y+h <= img.shape[0]:
            if detect_anne_hk_mod(img[y:y+h, x:x+w]):
                anne_hk_active = True
                if verbose:
                    print("    [Anne_HK resource panels] mod detected!")

    # 3. Process each element
    for name, coords in ui_map.get('elements', {}).items():
        value_str = ""
        x = int(coords['x_px'] * ui_scale)
        y = int(coords['y_px'] * ui_scale)
        w = int(coords['w_px'] * ui_scale)
        h = int(coords['h_px'] * ui_scale)
        
        # Mod Adjustment: Anne_HK villager numbers are larger and shifted upwards
        if anne_hk_active and name.endswith("_vils"):
            y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
            y = max(0, y - y_adj)
            h = h + y_adj
            
            # Expanded width for idle vils to handle 3 digits (e.g. 103)
            if name == "idle_vils":
                w_adj = int(w * ANNE_HK_IDLE_W_ADJUST_FACTOR)
                w = w + w_adj
        
        # Valid crop check
        if x+w > img.shape[1] or y+h > img.shape[0]:
            if verbose:
                print(f"Skipping {name}: Coordinates out of bounds")
            continue
            
        box_img = img[y:y+h, x:x+w].copy()
        
        # Select processing modes
        overlay_mode = False
        allow_yellow = False
        ignore_color = False

        if anne_hk_active and name.endswith("_vils"):
            ignore_color = True

        # Population Total Shortcut: Check for Housed overlay and enable Yellow Font detection
        if name == "population_total":
            overlay_mode = detect_housed_overlay(box_img)
            allow_yellow = True
            
            if overlay_mode:
                pop_color = "overlay"
            elif contains_yellow(box_img):
                pop_color = "yellow"
                
            if pop_color != "white" and verbose:
                state_desc = "overlay" if pop_color == "overlay" else "yellow text"
                print(f"    Housed state ({state_desc}) detected for {name}!")

        # Special Case: Idle Villagers are 0 if the icon is not active
        if name == "idle_vils":
            is_active = contains_red(box_img) if anne_hk_active else contains_yellow(box_img)
            
            if not is_active:
                value_str = "0"
                digits = []
                debug_details = ["ActiveCheck:0"]
            else:
                # Extract digits
                digits = extract_digits(box_img, ui_scale, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
                
                # Match digits
                value_str = ""
                debug_details = []
                for d_img in digits:
                    char, ssd, info = match_digit_to_template(d_img, templates)
                    value_str += char
                    debug_details.append(f"{char}({ssd:.1f}{info})")
                
                # If no digits found for idle_vils but we bypassed the yellow check, it's 0
                if not digits and anne_hk_active:
                    value_str = "0"
                    debug_details = ["AnneHK:0"]
        else:
            # Extract digits
            digits = extract_digits(box_img, ui_scale, overlay_mode=overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
            
            # Match digits
            value_str = ""
            debug_details = []
            for d_img in digits:
                char, ssd, info = match_digit_to_template(d_img, templates)
                value_str += char
                debug_details.append(f"{char}({ssd:.1f}{info})")
        
        if verbose:
            print(f"  {name:<20}: {value_str:<10} | Raw: {', '.join(debug_details)}")
        
        # Structure output to match expected_values.json (category -> sub_key)
        parts = name.split('_')
        category = parts[0]
        sub_key = parts[1] if len(parts) > 1 else "value"
        
        if category not in results:
            results[category] = {}
        
        # Special case: Split population into current and max (housing)
        if name == "population_total":
            if "/" in value_str:
                curr, housing = value_str.split('/', 1)
                results[category]['total'] = curr
                results[category]['housing'] = housing
            else:
                if verbose:
                    print(f"    WARNING: No slash found in {name}: '{value_str}'")
                results[category]['total'] = value_str
                results[category]['housing'] = ""
        else:
            results[category][sub_key] = value_str
    
    if 'population' not in results:
        results['population'] = {}
    
    results['population']['color'] = pop_color
    
    return results

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
    if ui_scale is None:
        print(f"[no anchor] Red UI reference not found (took {time_scale*1000:.2f}ms)")
        return {}
    print(f"Detected UI Scale: {ui_scale:.4f} (took {time_scale*1000:.2f}ms)")

    time_extract = 0
    time_match = 0

    # 2. Process frame
    results = process_frame(img, ui_map, templates, verbose=True)

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
