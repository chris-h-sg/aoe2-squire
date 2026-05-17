import cv2
import numpy as np
import json
import os
import argparse
import time

"""
AoE2 Squire Vision Pipeline (Proof of Concept)
----------------------------------------------
This script implements a scale-invariant vision pipeline for extracting 
game state from Age of Empires II: Definitive Edition.

ARCHITECTURAL PHILOSOPHY: "Detect First, Extract Second"
1. CALIBRATION: Detect UI scale and mod-specific geometry using fixed anchors (e.g., Red UI elements).
2. ANCHORING: Use a reference box (Wood) to find the vertical baseline of digits.
3. NORMALIZATION: Clip boxes to the detected baseline to ensure digits are vertically centered.
4. ROBUST OCR: Combine anchoring with standard vertical wiggle to maintain 100% accuracy across all resolutions.
"""

# ==========================================
# CONSTANTS & CONFIGURATION
# ==========================================

# --- Scale Detection ---
RED_PIXEL_MIN_COUNT = 12
RED_DIFF_THRESHOLD = 140     # Red must be this much higher than Green and Blue
ANCHOR_MIN_X = 0.5
ANCHOR_MAX_X = 0.95
ANCHOR_MIN_Y = 0.015
ANCHOR_MAX_Y = 0.035
UI_SCALE_MIN = 0.5
UI_SCALE_MAX = 2.0

# --- Mod Support (Anne_HK) ---
ANNE_HK_PIXEL_MIN_COUNT = 12
ANNE_HK_COLOR_DIFF_THRESHOLD = 150  # R and G must be this much higher than B
ANNE_HK_Y_ADJUST_FACTOR = 0.6       # Villager boxes expanded up by 60%
ANNE_HK_IDLE_W_ADJUST_FACTOR = 0.4   # Idle vils box expanded width by 40%
ANNE_HK_IDLE_RED_MIN = 150          # Red indicator threshold
ANNE_HK_IDLE_GB_MAX = 10            # Max Green/Blue for the red indicator

# --- Segmentation ---
SEG_GREY_TOLERANCE = 30
SEG_BRIGHTNESS_THRESHOLD = 100
SEG_REQUIRED_BRIGHTNESS = 230
BASELINE_MIN_AREA = 15

# --- Extraction Cleanup ---
OUT_GREY_TOLERANCE = 30
OUT_BRIGHTNESS_THRESHOLD_DEFAULT = 5
OUT_BRIGHTNESS_THRESHOLD_OVERLAY = 30

# --- Matching ---
WORKING_HEIGHT = 36
CANVAS_SIZE = 64
BLUR_SIGMA = 1.0
OFFSETS = [-1, 0, 1]

# --- OCR Tie-Breaker (0 vs 3/6/9) ---
TB_SSD_MARGIN = 0.20        # SSD difference must be within 20% for tie-breaker
TB_SYM_ZERO_MAX = 60        # Max symmetry score for a '0'
TB_SYM_ASYM_MIN = 40        # Min symmetry score for asymmetric digits
TB_ASYM_CHARS = {'3', '6', '9'}

# --- Scale Detection ---
BASELINE_MARGIN_PX = 263    # Standard 1080p right-side margin

# ==========================================
# 1. VISION UTILITIES (DETECTORS & FILTERS)
# ==========================================

def detect_ui_scale(img, baseline_margin=263):
    """
    Detects the UI scale by finding the right-most edge of the top-bar UI (red background).
    The distance from the right edge of the screen to this UI element is constant in 
    screen-space at 100% scale (263px). We use this to calculate the current scaling factor.
    """
    if img is None: return None
    h, w, _ = img.shape
    y_min_scan, y_max_scan = int(h * ANCHOR_MIN_Y), min(h - 1, int(h * ANCHOR_MAX_Y))
    x_min_scan, x_max_scan = int(w * ANCHOR_MIN_X), min(w - 1, int(w * ANCHOR_MAX_X))
    
    img_rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
    best_rightmost_x = None

    for y in range(y_min_scan, y_max_scan + 1):
        for x in range(x_min_scan, x_max_scan + 1):
            r, g, b = img_rgb[y, x]
            if (int(r) - int(g) >= RED_DIFF_THRESHOLD) and (int(r) - int(b) >= RED_DIFF_THRESHOLD):
                x_start = max(x_min_scan, x - 9)
                y_end = min(y_max_scan, y + 9)
                sub_area = img_rgb[y : y_end + 1, x_start : x + 1]
                
                r_chan, g_chan, b_chan = sub_area[:, :, 0].astype(np.int16), sub_area[:, :, 1].astype(np.int16), sub_area[:, :, 2].astype(np.int16)
                r_mask = (r_chan - g_chan >= RED_DIFF_THRESHOLD) & (r_chan - b_chan >= RED_DIFF_THRESHOLD)
                
                if np.sum(r_mask) >= RED_PIXEL_MIN_COUNT:
                    if best_rightmost_x is None or x > best_rightmost_x:
                        best_rightmost_x = x

    if best_rightmost_x is None: return None
    ui_scale = (w - best_rightmost_x) / baseline_margin
    return ui_scale if UI_SCALE_MIN <= ui_scale <= UI_SCALE_MAX else None

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
    if img is None or len(img.shape) < 3: return False
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

def apply_base_filter(img, grey_tol, brightness_thresh, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """Filters image by greyness and brightness to isolate text."""
    if img is None or img.size == 0: return np.zeros((1, 1), dtype=np.uint8)
    if ignore_color:
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        _, cleaned = cv2.threshold(gray, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if overlay_mode:
        h, w = img.shape[:2]
        if h < 3 or w < 3: return np.zeros((h, w), dtype=np.uint8)
        bg_color = img[2, 2].astype(np.int16)
        subtracted = np.clip(img.astype(np.int16) - bg_color, 0, 255).astype(np.uint8)
        gray_sub = cv2.cvtColor(subtracted, cv2.COLOR_BGR2GRAY)
        normalized = cv2.normalize(gray_sub, None, 0, 255, cv2.NORM_MINMAX)
        _, cleaned = cv2.threshold(normalized, brightness_thresh, 255, cv2.THRESH_TOZERO)
        return cleaned

    if len(img.shape) == 3:
        b, g, r = img[:, :, 0], img[:, :, 1], img[:, :, 2]
        max_val, min_val = np.max(img, axis=2).astype(np.int16), np.min(img, axis=2).astype(np.int16)
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
    """Produces a clean output image for the matcher."""
    thresh = OUT_BRIGHTNESS_THRESHOLD_OVERLAY if overlay_mode else OUT_BRIGHTNESS_THRESHOLD_DEFAULT
    return apply_base_filter(box_img, OUT_GREY_TOLERANCE, thresh, overlay_mode, allow_yellow, ignore_color)

# ==========================================
# 2. MATCHING LOGIC
# ==========================================

def calculate_h_symmetry(canvas):
    """
    Calculates a horizontal symmetry score.
    Used as a tie-breaker between '0' and asymmetric digits ('3', '6', '9') 
    when their SSD scores are too close to call.
    """
    flipped = cv2.flip(canvas, 1)
    diff = canvas - flipped
    return np.sum(diff * diff)

def prepare_canvas(img, target_h):
    """Scales, centers by bounding box, and blurs an image into a fixed canvas."""
    h_orig, w_orig = img.shape
    if h_orig == 0 or w_orig == 0: return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)

    scale = target_h / h_orig
    new_w = max(1, int(w_orig * scale))
    interp = cv2.INTER_CUBIC if scale > 1 else cv2.INTER_LINEAR
    upscaled = cv2.resize(img, (new_w, target_h), interpolation=interp)
    
    _, thresh = cv2.threshold(upscaled, 1, 255, cv2.THRESH_BINARY)
    coords = cv2.findNonZero(thresh)
    if coords is None: return np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.float32)
    
    x, y, w, h = cv2.boundingRect(coords)
    crop = upscaled[y:y+h, x:x+w]
    
    canvas = np.zeros((CANVAS_SIZE, CANVAS_SIZE), dtype=np.uint8)
    if h > CANVAS_SIZE or w > CANVAS_SIZE:
        scale_fit = min(CANVAS_SIZE/h, CANVAS_SIZE/w)
        crop = cv2.resize(crop, None, fx=scale_fit, fy=scale_fit, interpolation=cv2.INTER_AREA)
        h, w = crop.shape
        
    off_x, off_y = (CANVAS_SIZE - w) // 2, (CANVAS_SIZE - h) // 2
    canvas[off_y:off_y+h, off_x:off_x+w] = crop
    
    canvas_f = canvas.astype(np.float32) / 255.0
    return cv2.GaussianBlur(canvas_f, (0, 0), BLUR_SIGMA)

def load_templates(templates_dir):
    """Loads and pre-processes digit templates."""
    templates = {}
    if not os.path.exists(templates_dir): return templates
    for filename in os.listdir(templates_dir):
        if filename.endswith(".png"):
            char = filename.replace(".png", "").replace("slash", "/")
            img = cv2.imread(os.path.join(templates_dir, filename), cv2.IMREAD_GRAYSCALE)
            if img is not None:
                templates[char] = prepare_canvas(img, WORKING_HEIGHT)
    return templates

def match_digit_to_template(digit_img, processed_templates):
    """
    Matches an extracted digit against the template library using a two-stage process:
    
    1. Wiggle SSD: Calculates the Sum of Squared Differences (SSD) across a small 3x3 
       pixel grid (wiggle) to find the best alignment.
       
    2. Symmetry Tie-Breaker: If the match is a '0' but has low symmetry (as determined 
       by TB_SYM_ZERO_MAX), it favors asymmetric candidates like '3', '6', or '9'.
    """
    input_canvas = prepare_canvas(digit_img, WORKING_HEIGHT)
    shifted_inputs = []
    
    y_offsets = OFFSETS
    for dy in y_offsets:
        for dx in OFFSETS:
            M = np.float32([[1, 0, -dx], [0, 1, -dy]])
            shifted_inputs.append(cv2.warpAffine(input_canvas, M, (CANVAS_SIZE, CANVAS_SIZE)))

    all_matches = []
    for char, template_canvas in processed_templates.items():
        best_ssd = min(np.sum((si - template_canvas)**2) for si in shifted_inputs)
        all_matches.append((best_ssd, char))
    
    all_matches.sort()
    if not all_matches: return "?", 0.0, "No Templates"

    tb_info = ""
    if len(all_matches) > 1:
        best_ssd, best_char = all_matches[0]
        second_ssd, second_char = all_matches[1]
        
        is_conflict = (best_char == '0' and second_char in TB_ASYM_CHARS) or \
                      (best_char in TB_ASYM_CHARS and second_char == '0')
        
        if is_conflict and (second_ssd - best_ssd < best_ssd * TB_SSD_MARGIN):
            sym_score = calculate_h_symmetry(input_canvas)
            if (best_char == '0' and sym_score > TB_SYM_ZERO_MAX) or \
               (best_char in TB_ASYM_CHARS and sym_score < TB_SYM_ASYM_MIN):
                all_matches[0], all_matches[1] = all_matches[1], all_matches[0]
                tb_info = f"[SwapH:{sym_score:.1f}]"
            else:
                tb_info = f"[TrustH:{sym_score:.1f}]"
    
    return all_matches[0][1], all_matches[0][0], tb_info

def calculate_ssd_for_char(digit_img, char, templates):
    """Calculates the best Wiggle SSD for a specific character template."""
    if char not in templates: return float('inf')
    input_canvas = prepare_canvas(digit_img, WORKING_HEIGHT)
    template_canvas = templates[char]
    
    best_ssd = float('inf')
    y_offsets = OFFSETS
    for dy in y_offsets:
        for dx in OFFSETS:
            M = np.float32([[1, 0, -dx], [0, 1, -dy]])
            shifted = cv2.warpAffine(input_canvas, M, (CANVAS_SIZE, CANVAS_SIZE))
            ssd = np.sum((shifted - template_canvas)**2)
            if ssd < best_ssd:
                best_ssd = ssd
    return best_ssd

# ==========================================
# 3. SEGMENTATION ENGINE
# ==========================================

def get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold, 
                             overlay_mode=False, allow_yellow=False, ignore_color=False, 
                             offset_x=0, offset_y=0, connectivity=8, required_brightness=230):
    """
    Finds connected components in a box. If a component is unusually wide (indicating 
    overlapping digits), it recursively re-segments that sub-region with stricter 
    thresholds to force a split.
    """
    min_area = int(BASELINE_MIN_AREA * (ui_scale ** 2))
    req_bright = required_brightness if not ignore_color else 150
    
    seg = apply_base_filter(roi_bgr, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color)
    binary = (seg > 0).astype(np.uint8)
    num_labels, labels, stats, _ = cv2.connectedComponentsWithStats(binary, connectivity=connectivity)
    
    valid_indices = []
    for i in range(1, num_labels):
        if stats[i, cv2.CC_STAT_AREA] >= min_area:
            x, y, w, h = stats[i, cv2.CC_STAT_LEFT], stats[i, cv2.CC_STAT_TOP], stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]
            # Check if component has at least one bright pixel
            component_mask = (labels[y:y+h, x:x+w] == i)
            if np.any(component_mask):
                if np.max(seg[y:y+h, x:x+w][component_mask]) >= req_bright:
                    valid_indices.append(i)

    if not valid_indices: return []

    img_h, img_w = roi_bgr.shape[:2]
    if len(valid_indices) == 1:
        idx = valid_indices[0]
        w, h = stats[idx, cv2.CC_STAT_WIDTH], stats[idx, cv2.CC_STAT_HEIGHT]
        if w == img_w and h == img_h and (w + 2 > h):
            if brightness_threshold < 160:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold + 10, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)
            if grey_tolerance > 2:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance - 2, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, connectivity, required_brightness)
            if connectivity == 8:
                return get_components_recursive(roi_bgr, ui_scale, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x, offset_y, 4, required_brightness)
            return [{'x': offset_x, 'y': offset_y, 'w': w, 'h': h}]

    results = []
    for i in valid_indices:
        x, y, w, h = stats[i, 0], stats[i, 1], stats[i, 2], stats[i, 3]
        if w + 2 > h:
            isolated_roi = roi_bgr[y:y+h, x:x+w].copy()
            isolated_roi[labels[y:y+h, x:x+w] != i] = 0
            results.extend(get_components_recursive(isolated_roi, ui_scale, grey_tolerance, brightness_threshold, overlay_mode, allow_yellow, ignore_color, offset_x + x, offset_y + y, connectivity, required_brightness))
        else:
            results.append({'x': offset_x + x, 'y': offset_y + y, 'w': w, 'h': h})
    return results

def segment_box(box_img, out_img, ui_scale, overlay_mode=False, bright_threshold=230, allow_yellow=False, ignore_color=False):
    """
    Segments a processed box image into individual digit components.
    Uses recursive connected components analysis for robust extraction.
    """
    boxes = get_components_recursive(box_img, ui_scale, SEG_GREY_TOLERANCE, SEG_BRIGHTNESS_THRESHOLD, 
                                     overlay_mode, allow_yellow, ignore_color, required_brightness=bright_threshold)
    boxes.sort(key=lambda d: d['x'])
    
    digits = []
    for db in boxes:
        y, h = db['y'], db['h']
        digits.append(out_img[y : y+h, db['x'] : db['x']+db['w']])
    return digits, boxes

def extract_digits(box_img, ui_scale=1.0, overlay_mode=False, allow_yellow=False, ignore_color=False):
    """Wrapper for cleanup and segmentation (backwards compatibility)."""
    out_img = cleanup_box(box_img, overlay_mode, allow_yellow, ignore_color)
    digits, _ = segment_box(box_img, out_img, ui_scale, overlay_mode, allow_yellow=allow_yellow, ignore_color=ignore_color)
    return digits

# ==========================================
# 4. VISION PIPELINE
# ==========================================

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
        Calibrates the pipeline by detecting:
        1. UI Scale: Using the red top-bar anchor.
        2. Active Mods: Checking for 'Anne_HK' color signatures.
        3. Vertical Baselines: Detecting the exact height and Y-offset of digits 
           using 'wood_total' and 'wood_vils' as reference anchors.
        """
        self.ui_scale = detect_ui_scale(img, self.ui_map.get('baseline_margin', BASELINE_MARGIN_PX))
        if self.ui_scale is None: 
            self.ui_scale = 1.0 # Fallback
            return False
        
        # Mod detection
        wood_vils_coords = self.ui_map.get('elements', {}).get('wood_vils')
        if wood_vils_coords:
            x, y, w, h = self._get_raw_coords("wood_vils", wood_vils_coords)
            if detect_anne_hk_mod(img, x, y, w, h):
                self.anne_hk_active = True
        
        # Reference metrics from wood_total (Primary Resource Digits)
        wood_total_coords = self.ui_map.get('elements', {}).get('wood_total')
        if wood_total_coords:
            self.ref_h, self.ref_y = self._detect_baseline(img, "wood_total", wood_total_coords)
            
        # Reference metrics from wood_vils (Villager Digits)
        if wood_vils_coords:
            self.vil_ref_h, self.vil_ref_y = self._detect_baseline(img, "wood_vils", wood_vils_coords)
            
        return True

    def _detect_baseline(self, img, name, coords):
        """
        Finds the vertical baseline (height and Y-offset) of digits in a box.
        Uses the union of all detected digit spans to ensure the baseline reflects 
        the tallest digit, preventing over-clipping when short digits (like '1') 
        are present in the reference box.
        """
        x, y, w, h = self._get_raw_coords(name, coords)
        
        # Apply mod adjustments if needed
        if self.anne_hk_active and name.endswith("_vils"):
            y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
            y, h = max(0, y - y_adj), h + y_adj
            
        box_img = img[y:y+h, x:x+w].copy()
        ignore_color = (self.anne_hk_active and name.endswith("_vils"))
        out_img = cleanup_box(box_img, ignore_color=ignore_color)
        digits, boxes = segment_box(box_img, out_img, self.ui_scale, bright_threshold=self.bright_thresh, ignore_color=ignore_color)
        
        if boxes:
            y_min = min(b['y'] for b in boxes)
            y_max = max(b['y'] + b['h'] for b in boxes)
            return (y_max - y_min), y_min
        return None, None

    def _get_raw_coords(self, name, coords):
        return (int(coords['x_px'] * self.ui_scale), int(coords['y_px'] * self.ui_scale),
                int(coords['w_px'] * self.ui_scale), int(coords['h_px'] * self.ui_scale))

    def get_element_coords(self, name, coords):
        """Calculates final coordinates with mods and vertical clipping."""
        x, y, w, h = self._get_raw_coords(name, coords)
        
        # 1. Mod Adjustment
        if self.anne_hk_active and name.endswith("_vils"):
            y_adj = int(h * ANNE_HK_Y_ADJUST_FACTOR)
            y, h = max(0, y - y_adj), h + y_adj
            if name == "idle_vils": w += int(w * ANNE_HK_IDLE_W_ADJUST_FACTOR)
        
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
            y, h, clipped = y + target_ref[0], target_ref[1], True
            
        return x, y, w, h, clipped

    def process_box(self, img, name, coords):
        """Orchestrates extraction, cleanup, and segmentation for a box."""
        x, y, w, h, clipped = self.get_element_coords(name, coords)
        if x+w > img.shape[1] or y+h > img.shape[0]: return None, [], [], clipped
        
        box_img = img[y:y+h, x:x+w].copy()
        overlay_mode, allow_yellow, ignore_color = False, False, False
        if self.anne_hk_active and name.endswith("_vils"): ignore_color = True
        if name == "population_total": overlay_mode, allow_yellow = detect_housed_overlay(box_img), True

        out_img = cleanup_box(box_img, overlay_mode, allow_yellow, ignore_color)
        
        if name == "idle_vils":
            if not (contains_red(box_img) if self.anne_hk_active else contains_yellow(box_img)):
                return box_img, [], [], clipped
                
        digits, digit_boxes = segment_box(box_img, out_img, self.ui_scale, overlay_mode, 
                                          self.bright_thresh, allow_yellow, ignore_color)
        return box_img, digits, digit_boxes, clipped

    def run(self, img, templates, verbose=False):
        """
        Orchestrates the full extraction for a single frame.
        Iterates through all elements defined in the UI map, extracts their images, 
        performs OCR, and maps the results into a structured JSON-like dictionary.
        """
        if not self.calibrate(img):
            if verbose: print("[no anchor] Red UI reference not found.")
            return {}

        if verbose:
            if self.anne_hk_active: print("    [Anne_HK resource panels] mod detected!")
            if self.ref_h: print(f"    Reference digit height: {self.ref_h}px")

        results = {}
        pop_color = "white"
        
        for name, coords in self.ui_map.get('elements', {}).items():
            box_img, digits, _, clipped = self.process_box(img, name, coords)
            if box_img is None: continue

            # OCR Matching
            match_results = []
            
            for d_img in digits:
                char, ssd, info = match_digit_to_template(d_img, templates)
                match_results.append({'char': char, 'ssd': ssd, 'info': info, 'img': d_img})
                
            # Population Slash Heuristic: Force exactly one slash if missing
            if name == "population_total" and digits:
                if not any(m['char'] == "/" for m in match_results):
                    slash_diffs = [calculate_ssd_for_char(m['img'], "/", templates) - m['ssd'] for m in match_results]
                    best_idx = np.argmin(slash_diffs)
                    match_results[best_idx]['char'] = "/"
                    match_results[best_idx]['info'] += f"[Forced/ d={slash_diffs[best_idx]:.1f}]"

            # Reconstruct value string and logging info
            value_str = "".join(m['char'] for m in match_results)
            debug_details = [f"{m['char']}({m['ssd']:.1f}{m['info']})" for m in match_results]
                
            if name == "idle_vils" and not digits:
                value_str, debug_details = "0", ["Shortcut:0"]
                
            if name == "population_total":
                if detect_housed_overlay(box_img): pop_color = "overlay"
                elif contains_yellow(box_img): pop_color = "yellow"
                if verbose and pop_color != "white": print(f"    Housed state ({pop_color}) detected!")

            if verbose: print(f"  {name:<20}: {value_str:<10} | Raw: {', '.join(debug_details)}")
            
            # Map elements into structured results (category.key)
            parts = name.split('_')
            category, sub_key = parts[0], (parts[1] if len(parts) > 1 else "value")
            if category not in results: results[category] = {}
            
            if name == "population_total":
                curr, housing = value_str.split('/', 1) if "/" in value_str else (value_str, "")
                results[category]['total'], results[category]['housing'] = curr, housing
            else:
                results[category][sub_key] = value_str
        
        if 'population' not in results: results['population'] = {}
        results['population']['color'] = pop_color
        return results

def process_frame(frame, ui_map, templates, verbose=False):
    """
    Legacy wrapper for video processing.
    Instantiates a one-off pipeline to process a single frame.
    """
    pipeline = ExtractorPipeline(ui_map)
    return pipeline.run(frame, templates, verbose=verbose)

def process_image(image_path, ui_map, templates):
    start_time = time.time()
    img = cv2.imread(image_path)
    if img is None: return None

    proc_start = time.time()
    pipeline = ExtractorPipeline(ui_map)
    results = pipeline.run(img, templates, verbose=True)
    
    proc_time = time.time() - proc_start
    total_time = time.time() - start_time
    print(f"\nTiming: Core Logic: {proc_time*1000:.2f}ms | Total: {total_time*1000:.2f}ms")
    return results

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", type=str)
    parser.add_argument("--bright-thresh", type=int, default=230)
    args = parser.parse_args()
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(script_dir, '..', 'ui_map.json'), 'r') as f:
        ui_map = json.load(f)
    
    templates = load_templates(os.path.join(script_dir, 'templates', 'enormous_numbers'))
    print(f"Loaded {len(templates)} templates.")

    image_path = args.image or os.path.join(script_dir, '..', 'test_bench', 'aoe2_16x9.png')
    process_image(image_path, ui_map, templates)

if __name__ == "__main__":
    main()
