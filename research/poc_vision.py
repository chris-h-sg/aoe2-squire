
import cv2
import numpy as np
import json
import os

# ===== CONFIGURATION =====
COLOR_TOLERANCE = 30   # Logic: Max diff between R,G,B channels. Higher = more permissive.
BINARY_THRESHOLD = 120 # Logic: Brightness cutoff. Lower = captures dimmer pixels.
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
    # Nonzero returns (row_idxs, col_idxs)
    y_idxs, x_idxs = np.nonzero(mask)
    
    if len(x_idxs) == 0:
        return 0
        
    # Find the rightmost pixel (max x)
    max_x = np.max(x_idxs)
    
    # Return distance from right edge
    return w - max_x



def perform_ocr(cleaned_crop, templates, region_name="unknown", save_candidates=False, ui_scale=1.0):
    """Identifies digits in a cleaned crop using 1:1 template matching."""
    if not templates:
        return ""
    
    print(f"\n  === OCR for {region_name} (Scale {ui_scale:.2f}) ===")

    rects = []
    contours, _ = cv2.findContours(cleaned_crop, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    for cnt in contours:
        x, y, w, h = cv2.boundingRect(cnt)
        ch, cw = cleaned_crop.shape
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
    
    found_chars = []
    blob_idx = 0
    for x_c, y_c, w_c, h_c in filtered:
        digit_blob = cleaned_crop[y_c:y_c+h_c, x_c:x_c+w_c]
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
            # SCALE THE TEMPLATE
            t_img = template
            if ui_scale != 1.0:
                t_h, t_w = template.shape
                new_w = max(1, int(t_w * ui_scale))
                new_h = max(1, int(t_h * ui_scale))
                t_img = cv2.resize(template, (new_w, new_h), interpolation=cv2.INTER_NEAREST)
            
            t_h, t_w = t_img.shape
            max_h, max_w = max(t_h, h_c), max(t_w, w_c)
            t_pad = np.zeros((max_h, max_w), dtype=np.uint8)
            b_pad = np.zeros((max_h, max_w), dtype=np.uint8)
            
            t_pad[(max_h-t_h)//2 : (max_h-t_h)//2 + t_h, (max_w-t_w)//2 : (max_w-t_w)//2 + t_w] = t_img
            b_pad[(max_h-h_c)//2 : (max_h-h_c)//2 + h_c, (max_w-w_c)//2 : (max_w-w_c)//2 + w_c] = digit_blob
            
            intersection = np.logical_and(t_pad, b_pad).sum()
            union = np.logical_or(t_pad, b_pad).sum()
            if union > 0:
                score = intersection / union
                all_matches.append((score, char))
        
        all_matches.sort(reverse=True)
        top_matches = all_matches[:3]
        
        if top_matches:
            best_match, best_char = top_matches[0]
            print(f"    Blob {digit_blob.shape} at x={x_c} Leaderboard:")
            for score, char in top_matches:
                print(f"      - '{char}': {score:.2f}")
                    
            if best_match > 0.4: 
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
            text = perform_ocr(cleaned, templates, current_label, 
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
    args = parser.parse_args()
    
    debug_options = {
        'save_red_mask': args.save_red_mask,
        'save_debug_image': args.save_debug_image,
        'save_candidates': args.save_candidates,
        'save_crops': args.save_crops
    }

    test_bench_dir = "test_bench"
    if not os.path.exists(test_bench_dir):
        print("Test bench directory not found!")
        return

    config = load_config() 
    templates = load_templates(os.path.join("research", "templates", "resource_numbers"))

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
