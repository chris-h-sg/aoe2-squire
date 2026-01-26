
import cv2
import numpy as np
import json
import os

try:
    import pytesseract
except ImportError:
    pytesseract = None

# ===== CONFIGURATION =====
COLOR_TOLERANCE = 30   # Logic: Max diff between R,G,B channels. Higher = more permissive.
BINARY_THRESHOLD = 120 # Logic: Brightness cutoff. Lower = captures dimmer pixels.

# Tesseract Search Paths (Common Windows locations)
TESSERACT_SEARCH_PATHS = [
    r"C:\Program Files\Tesseract-OCR\tesseract.exe",
    r"C:\Users\{}\AppData\Local\Tesseract-OCR\tesseract.exe".format(os.getlogin()),
    r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe"
]

def init_tesseract():
    if pytesseract is None:
        return False
    
    # Check if tesseract is already in path
    import subprocess
    try:
        subprocess.run(["tesseract", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return True
    except FileNotFoundError:
        pass

    # Try common paths
    for path in TESSERACT_SEARCH_PATHS:
        if os.path.exists(path):
            pytesseract.pytesseract.tesseract_cmd = path
            return True
    
    return False

# =========================

def load_config():
    with open('ui_map.json', 'r') as f:
        return json.load(f)

def load_templates(templates_dir):
    """Loads digit templates from the templates directory."""
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
                templates[char] = img
    return templates

def process_stage_2_filter(crop):
    """Stage 2: Filter out non-grayscale pixels (colored UI elements)."""
    # Convert to int16 to avoid uint8 overflow/underflow during subtraction
    # e.g., 226 - 229 should be -3, not 253
    crop_int = crop.astype(np.int16)
    b, g, r = cv2.split(crop_int)
    
    # Calculate max difference between any two channels
    max_diff = np.maximum(np.maximum(np.abs(r - g), np.abs(g - b)), np.abs(r - b))
    
    # Mask: True if pixel is COLORED (diff > tolerance)
    non_grayscale_mask = max_diff > COLOR_TOLERANCE
    
    # Set non-grayscale pixels to black
    filtered = crop.copy()
    filtered[non_grayscale_mask] = [0, 0, 0]
    return filtered

def process_stage_3_clean(filtered_crop):
    gray = cv2.cvtColor(filtered_crop, cv2.COLOR_BGR2GRAY)
    _, thresh = cv2.threshold(gray, BINARY_THRESHOLD, 255, cv2.THRESH_BINARY)
    return thresh

    return thresh

# ===== CONFIGURATION =====
# Red Pixel Filter config (BGR)
RED_MASK_LOWER = np.array([0, 0, 201])    # B<X, G<X, R>200 (OpenCV uses BGR)
RED_MASK_UPPER = np.array([60, 60, 255])
# =========================


def get_ui_right_margin(img):
    """
    Determines the distance from the right screen edge to the rightmost red UI element.
    Used for detecting UI Scale.
    """
    if img is None:
        return 0

    h, w, _ = img.shape
    
    # Analyze only the top 20% of the screen
    top_h = int(h * 0.2)
    crop = img[0:top_h, :]

    # Create mask for red pixels: R > 200, G < 60, B < 60
    # InRange is faster than manual channel splitting
    # Note: OpenCV is BGR
    mask = cv2.inRange(crop, RED_MASK_LOWER, RED_MASK_UPPER)
    
    # Find coordinates of all non-zero (white) pixels in the mask
    # Use a smaller cluster requirement for robustness
    y_idxs, x_idxs = np.nonzero(mask)
    
    if len(x_idxs) < 5: 
        return 0
        
    # Find the rightmost pixel (max x)
    max_x = np.max(x_idxs)
    
    # Return distance from right edge
    return w - max_x



def perform_ocr_tesseract(crop, region_name="unknown", whitelist="0123456789/", debug_options=None):
    """Identifies text in a crop using Tesseract OCR."""
    if pytesseract is None:
        print("  [!] Pytesseract not installed. Install with: pip install pytesseract")
        return ""
    
    if debug_options is None:
        debug_options = {}

    # Determine mode for filename
    mode = "binary"
    if debug_options.get('ocr_hybrid', False):
        mode = "hybrid"
    elif debug_options.get('ocr_raw', False):
        mode = "raw"
    elif debug_options.get('ocr_filtered', False):
        mode = "filtered"

    # Scale up 4x for better Tesseract performance on small fonts
    if len(crop.shape) == 3:
        # Convert to grayscale first to handle inversion properly
        gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
        h, w = gray.shape
    else:
        gray = crop
        h, w = gray.shape
        
    rescaled = cv2.resize(gray, (w * 4, h * 4), interpolation=cv2.INTER_CUBIC)
    
    # --- IMAGE SMOOTHING ---
    # Slight blur helps Tesseract handle pixel-art fonts by smoothing edges
    rescaled = cv2.GaussianBlur(rescaled, (3, 3), 0)
    
    # --- CONTRAST ENHANCEMENT ---
    # Normalize to ensure white text is actually white and background is dark
    rescaled = cv2.normalize(rescaled, None, 0, 255, cv2.NORM_MINMAX)
    
    # Tesseract configuration: 
    # --psm 8: Treat the image as a single word.
    # --dpi 300: Hint the resolution for better scaling.
    custom_config = f'--psm 8 --dpi 300 -c tessedit_char_whitelist={whitelist}'
    
    if debug_options.get('ocr_hints', False):
        words_path = os.path.abspath(os.path.join("research", "tess_config", "words.txt"))
        patterns_path = os.path.abspath(os.path.join("research", "tess_config", "patterns.txt"))
        # Force Tesseract to prioritize user dictionary/patterns
        custom_config += ' -c load_system_dawg=0 -c load_freq_dawg=0'
        
        if os.path.exists(words_path):
            custom_config += f' --user-words "{words_path}"'
        if os.path.exists(patterns_path):
            custom_config += f' --user-patterns "{patterns_path}"'
    
    # Pad the crop
    padded = cv2.copyMakeBorder(rescaled, 20, 20, 20, 20, cv2.BORDER_CONSTANT, value=[0, 0, 0])
    
    # ALWAYS invert to black on white for Tesseract's LSTM engine
    ocr_input = cv2.bitwise_not(padded)
    
    # ALWAYS save the exact input Tesseract sees (for research/debugging)
    out_dir = os.path.join("research", "output", "ocr_input", mode)
    os.makedirs(out_dir, exist_ok=True)
    cv2.imwrite(os.path.join(out_dir, f"{region_name}.png"), ocr_input)

    try:
        text = pytesseract.image_to_string(ocr_input, config=custom_config).strip()
        print(f"  Tesseract result for {region_name}: '{text}'")
        return text
    except Exception as e:
        print(f"  [!] Tesseract error: {e}")
        return ""

def perform_ocr(crop, templates, region_name="unknown", save_candidates=False, ui_scale=1.0):
    """Identifies digits in a crop using Template Matching with Correlation."""
    if not templates:
        return ""
    
    # Ensure we are working with single-channel grayscale for detection
    if len(crop.shape) == 3:
        gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
    else:
        gray = crop

    # Step 1: Create a binary version just for layout (contour) detection
    _, layout_mask = cv2.threshold(gray, BINARY_THRESHOLD, 255, cv2.THRESH_BINARY)

    print(f"\n  === Template OCR for {region_name} (Scale {ui_scale:.2f}) ===")

    rects = []
    contours, _ = cv2.findContours(layout_mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    for cnt in contours:
        x, y, w, h = cv2.boundingRect(cnt)
        ch, cw = gray.shape
        if w > 1 and h > 5 and w < cw * 0.9 and h < ch * 0.9:
            rects.append([x, y, w, h])
    
    print(f"  Found {len(rects)} raw blobs")
    
    # Filter overlapping/nested rectangles (keep largest)
    rects.sort(key=lambda r: r[2]*r[3], reverse=True)
    filtered = []
    for r in rects:
        is_nested = False
        r_cx, r_cy = r[0] + r[2]//2, r[1] + r[3]//2
        for f in filtered:
            if r_cx >= f[0] and r_cx <= f[0]+f[2] and r_cy >= f[1] and r_cy <= f[1]+f[3]:
                is_nested = True
                break
        if not is_nested:
            filtered.append(r)
    
    print(f"  After filtering: {len(filtered)} unique blobs")
    filtered.sort() # Sort left-to-right

    # SPLIT LOGIC: HUD fonts are tight; sometimes digits touch.
    # If a blob is much wider than it is tall, it's likely multiple digits.
    split_filtered = []
    for x, y, w, h in filtered:
        # Standard digits are roughly 8-10px wide at 11-12px height.
        # If width > 1.4 * height, it's probably 2+ digits.
        if w > h * 1.4:
            num_digits = int(round(w / (h * 0.8))) # Estimate digit count
            print(f"    [Split] Large blob {w}x{h} detected. Estimating {num_digits} digits.")
            dw = w / num_digits
            for i in range(num_digits):
                split_filtered.append([int(x + i*dw), y, int(dw), h])
        else:
            split_filtered.append([x, y, w, h])
    
    filtered = split_filtered
    print(f"  After splitting: {len(filtered)} unique blobs")
    
    # NEW: Handle templates of any resolution (e.g., 150px "huge" templates)
    # Target height at 1080p (ui_scale 1.0) is approx 12px as per user specs.
    # We use '0' as our height reference to calculate the normalization factor.
    ref_digit = templates.get('0')
    normalization_factor = 1.0
    if ref_digit is not None:
        template_h = ref_digit.shape[0]
        # If the template is larger than baseline, normalize it down to 12px
        if template_h > 20: 
            normalization_factor = 12.0 / template_h
            print(f"  Normalization Factor: {normalization_factor:.4f} (Source: {template_h}px -> Target: 12px)")

    # Total scale: normalization (to baseline) * current screen UI scale
    effective_scale = normalization_factor * ui_scale
    if effective_scale != 1.0:
        print(f"  Effective Template Scale: {effective_scale:.4f}")

    # HYBRID SCALING: We upscale the small screenshot blob to a "working resolution"
    # to allow the high-res templates to contribute more detail.
    WORKING_HEIGHT = 36 # 3x the baseline height
    
    found_chars = []
    blob_idx = 0
    for x_c, y_c, w_c, h_c in filtered:
        # 1. Capture the raw grayscale blob
        digit_blob_raw = gray[y_c:y_c+h_c, x_c:x_c+w_c]
        
        # 2. Upscale the blob to the working resolution
        # We use CUBIC to keep the anti-aliased edges smooth
        scale_up = WORKING_HEIGHT / h_c
        new_bw = max(1, int(w_c * scale_up))
        digit_blob = cv2.resize(digit_blob_raw, (new_bw, WORKING_HEIGHT), interpolation=cv2.INTER_CUBIC)
        
        best_char = "?"
        best_match = 0
        
        # Save candidates for inspection
        if save_candidates:
            candidates_dir = os.path.join("research", "output", "digit_candidates")
            os.makedirs(candidates_dir, exist_ok=True)
            cv2.imwrite(os.path.join(candidates_dir, f"{region_name}_blob{blob_idx}.png"), digit_blob)
        
        # DEBUG: Find Top 3 Leaderboard
        all_matches = []
        for char, template in templates.items():
            # SCALE THE TEMPLATE down to the WORKING_HEIGHT
            # Keep grayscale intensity for anti-aliasing!
            t_h_orig, t_w_orig = template.shape
            t_scale = WORKING_HEIGHT / t_h_orig
            t_new_w = max(1, int(t_w_orig * t_scale))
            
            t_img = cv2.resize(template, (t_new_w, WORKING_HEIGHT), interpolation=cv2.INTER_AREA)
            
            # Use Template Matching (Correlation) instead of Binary IoU
            # Correlation handles anti-aliased edges and intensity much better than logic-gate IoU.
            res = cv2.matchTemplate(digit_blob, t_img, cv2.TM_CCOEFF_NORMED)
            _, max_val, _, _ = cv2.minMaxLoc(res)
            all_matches.append((max_val, char))
        
        all_matches.sort(reverse=True)
        top_matches = all_matches[:3]
        
        if top_matches:
            best_match, best_char = top_matches[0]
            print(f"    Blob {digit_blob.shape} at x={x_c} Leaderboard:")
            for score, char in top_matches:
                print(f"      - '{char}': {score:.2f}")
                    
            if best_match > 0.6: # Correlation threshold is usually higher than IoU
                found_chars.append(best_char)
                print(f"      [OK] ACCEPTED")
            else:
                print(f"      [X] REJECTED: Confidence {best_match:.2f} too low")
        
        blob_idx += 1
    
    result = "".join(found_chars)
    print(f"  Final result: '{result}'")
    return result


def analyze_screenshot(image_path, config, templates, debug_options=None):
    return analyze_screenshot_return_results(image_path, config, templates, debug_options)

def analyze_screenshot_return_results(image_path, config, templates, debug_options=None):
    if debug_options is None:
        debug_options = {}

    print(f"Analyzing: {image_path}")
    img = cv2.imread(image_path)
    if img is None:
        print(f"Failed to load {image_path}")
        return

    h, w, _ = img.shape
    print(f"  Resolution: {w}x{h}")
    
    # 0. Calculate UI Scale
    baseline_margin = config.get('baseline_margin', 265.0)
    current_margin = get_ui_right_margin(img)
    if current_margin == 0:
        print("  Warning: Could not detect UI margin. Falling back to scale 1.0.")
        ui_scale = 1.0
    else:
        ui_scale = current_margin / baseline_margin
    
    print(f"  Scale Factor: {ui_scale:.4f} (Margin: {current_margin}, Baseline: {baseline_margin})")

    # Optional: Save Red Mask
    if debug_options.get('save_red_mask', False):
        mask = cv2.inRange(img, RED_MASK_LOWER, RED_MASK_UPPER)
        red_out = os.path.join("research", "output", "red_mask_" + os.path.basename(image_path))
        cv2.imwrite(red_out, mask)
        print(f"  Saved red mask to {red_out}")

    elements = config.get('elements', {})
    
    # 1. Setup Directories
    base_out = os.path.join("research", "output")
    dirs = {
        "raw": os.path.join(base_out, "crops", "raw"),
        "filtered": os.path.join(base_out, "crops", "filtered"),
        "clean": os.path.join(base_out, "crops", "clean"),
        "candidates": os.path.join(base_out, "digit_candidates")
    }
    
    # Only create directories if we intend to save files
    should_save_crops = debug_options.get('save_crops', False)
    if should_save_crops:
        for d in dirs.values():
            os.makedirs(d, exist_ok=True)

    results = {}
    debug_drawings = [] 

    for name, data in elements.items():
        try:
            # APPLY SCALING TO BASE COORDINATES
            raw_x, raw_y = data['x_px'], data['y_px']
            raw_w, raw_h = data['w_px'], data['h_px']
            
            rx = int(raw_x * ui_scale)
            ry = int(raw_y * ui_scale)
            rw = int(raw_w * ui_scale)
            rh = int(raw_h * ui_scale)
            
            # Apply a tiny margin for larger boxes to avoid border noise
            if rw > 50 or rh > 30:
                margin = int(2 * ui_scale)
                rx += margin
                ry += margin
                rw -= 2 * margin
                rh -= 2 * margin

            # Group results by resource name (e.g. "wood_total" -> "wood": {"total": ...})
            base_name = name
            sub_key = "value"
            if "_" in name:
                parts = name.split("_")
                base_name = parts[0]
                sub_key = parts[1]

            if base_name not in results:
                results[base_name] = {}
            
            current_label = name
            
            # --- STAGE 1: RAW CROP ---
            rw = max(1, rw)
            rh = max(1, rh)
            crop = img[ry:ry+rh, rx:rx+rw].copy()
            if should_save_crops:
                cv2.imwrite(os.path.join(dirs["raw"], f"{current_label}.png"), crop)
            
            # --- STAGE 2: FILTERED (Grayscale Logic) ---
            filtered = process_stage_2_filter(crop)
            if should_save_crops:
                cv2.imwrite(os.path.join(dirs["filtered"], f"{current_label}.png"), filtered)
            
            # --- STAGE 3: CLEAN (Binary Threshold) ---
            cleaned = process_stage_3_clean(filtered)
            if should_save_crops:
                cv2.imwrite(os.path.join(dirs["clean"], f"{current_label}.png"), cleaned)
            
            # --- FINAL: OCR ---
            engine = debug_options.get('ocr_engine', 'template')
            if engine == 'tesseract':
                if debug_options.get('ocr_hybrid', False):
                    # Hybrid logic: Villager-specific counts use filtered, others use raw
                    if current_label in ["population_vils", "idle_vils"]:
                        ocr_source = filtered
                    else:
                        ocr_source = crop
                elif debug_options.get('ocr_raw', False):
                    ocr_source = crop
                elif debug_options.get('ocr_filtered', False):
                    ocr_source = filtered
                else:
                    ocr_source = filtered
                
                # Determine whitelist: Only population_total gets the slash
                whitelist = "0123456789"
                if current_label == "population_total":
                    whitelist += "/"
                    
                text = perform_ocr_tesseract(ocr_source, current_label, whitelist, debug_options)
            else:
                # Use FILTERED crop (grayscale) instead of CLEANED (binary) for Template OCR
                text = perform_ocr(filtered, templates, current_label, 
                                 save_candidates=debug_options.get('save_candidates', False),
                                 ui_scale=ui_scale)
            
            # Use sub_key (derived from name split) for results
            results[base_name][sub_key] = text
            
            debug_drawings.append(('rect', (rx, ry), (rx + rw, ry + rh), (0, 255, 255), 1))
            debug_drawings.append(('text', current_label, (rx, ry - 2)))

        except KeyError as e:
            print(f"    Missing pixel key {e} for element {name}")

    print("\n--- OCR Results (Baseline) ---")
    print(json.dumps(results, indent=2))

    if debug_options.get('save_debug_image', False):
        # Draw Debug Visuals
        for cmd in debug_drawings:
            if cmd[0] == 'rect':
                cv2.rectangle(img, cmd[1], cmd[2], cmd[3], cmd[4])
            elif cmd[0] == 'line':
                cv2.line(img, cmd[1], cmd[2], cmd[3], cmd[4])
            elif cmd[0] == 'text':
                cv2.putText(img, cmd[1], cmd[2], cv2.FONT_HERSHEY_SIMPLEX, 0.3, (0, 255, 255), 1)

        output_path = os.path.join("research", "output", "debug_" + os.path.basename(image_path))
        cv2.imwrite(output_path, img)
        print(f"  Saved debug output to {output_path}")
    
    return results

def main():
    import argparse
    parser = argparse.ArgumentParser(description="Analyze RTS screenshot for resource data")
    parser.add_argument("--save-red-mask", action="store_true", help="Output the isolated red mask image")
    parser.add_argument("--save-debug-image", action="store_true", help="Output the main debug image with bounding boxes")
    parser.add_argument("--save-candidates", action="store_true", help="Output individual digit blobs found during OCR")
    parser.add_argument("--save-crops", action="store_true", help="Output raw, filtered, and clean crop stages")
    parser.add_argument("--image", type=str, help="Specific image filename to analyze (default: runs on aoe2_16x9.png if not specified)")
    parser.add_argument("--ocr-engine", type=str, choices=['template', 'tesseract'], default='template', help="OCR engine to use (default: template)")
    parser.add_argument("--ocr-raw", action="store_true", help="Feed raw crops to OCR engine instead of cleaned binary")
    parser.add_argument("--ocr-filtered", action="store_true", help="Feed color-filtered (non-grayscale removed) crops to OCR engine without thresholding")
    parser.add_argument("--ocr-hybrid", action="store_true", help="Use filtered for vils, raw for totals (Tesseract only)")
    parser.add_argument("--ocr-hints", action="store_true", help="Provide Tesseract with custom word/pattern hints from research/tess_config/")
    parser.add_argument("--save-ocr-input", action="store_true", help="Save the exact images being passed to the OCR engine")
    args = parser.parse_args()
    
    debug_options = {
        'save_red_mask': args.save_red_mask,
        'save_debug_image': args.save_debug_image,
        'save_candidates': args.save_candidates,
        'save_crops': args.save_crops,
        'ocr_engine': args.ocr_engine,
        'ocr_raw': args.ocr_raw,
        'ocr_filtered': args.ocr_filtered,
        'ocr_hybrid': args.ocr_hybrid,
        'ocr_hints': args.ocr_hints,
        'save_ocr_input': args.save_ocr_input
    }

    if args.ocr_engine == 'tesseract':
        if not init_tesseract():
            print("Error: Tesseract OCR requested but tesseract executable not found.")
            print("Please install Tesseract and ensure it's in your PATH, or in C:\\Program Files\\Tesseract-OCR\\")
            return

    test_bench_dir = "test_bench"
    if not os.path.exists(test_bench_dir):
        print("Test bench directory not found!")
        return

    config = load_config() 
    templates = load_templates(os.path.join("research", "templates", "huge_numbers"))

    # Determine which files to process
    if args.image:
        target_files = [args.image]
    else:
        # Default behavior: Run on the standard baseline 16x9 image
        target_files = ["aoe2_16x9.png"]

    for filename in target_files:
        path = os.path.join(test_bench_dir, filename)
        if os.path.exists(path):
            analyze_screenshot(path, config, templates, debug_options)
        else:
            print(f"Skipping {filename}: File not found in {test_bench_dir}")

if __name__ == "__main__":
    main()
