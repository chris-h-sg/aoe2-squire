
import cv2
import numpy as np
import json
import os

# ===== CONFIGURATION =====
COLOR_TOLERANCE = 30   # Logic: Max diff between R,G,B channels. Higher = more permissive.
BINARY_THRESHOLD = 110 # Logic: Brightness cutoff. Lower = captures dimmer pixels.
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
    """Stage 3: Convert to binary black & white for OCR."""
    gray = cv2.cvtColor(filtered_crop, cv2.COLOR_BGR2GRAY)
    _, thresh = cv2.threshold(gray, BINARY_THRESHOLD, 255, cv2.THRESH_BINARY)
    return thresh

def get_resource_panel_height(img):
    """
    Determines the height of the resource panel by finding the top and bottom borders.
    Uses a vertical strip crop and looks for black lines.
    """
    if img is None:
        return 0

    h, w, _ = img.shape
    
    # Configuration
    CROP_WIDTH = 20
    CROP_X_OFFSET = 3
    BRIGHTNESS_THRESHOLD = 20
    BLACK_PIXEL_COUNT_THRESHOLD = 18

    # 1. Crop the leftmost pixels of the entire image height with offset
    left_crop = img[0:h, CROP_X_OFFSET : CROP_X_OFFSET + CROP_WIDTH]
    
    # 2. Set all pixels below brightness threshold to 0 and all above to 255
    gray = cv2.cvtColor(left_crop, cv2.COLOR_BGR2GRAY)
    _, cleaned = cv2.threshold(gray, BRIGHTNESS_THRESHOLD, 255, cv2.THRESH_BINARY)

    # 3. Analyze for black lines
    black_pixel_counts = np.sum(cleaned == 0, axis=1)
    matching_rows = np.where(black_pixel_counts > BLACK_PIXEL_COUNT_THRESHOLD)[0]

    if len(matching_rows) == 0:
        return 0

    # Group consecutive lines
    lines = []
    if len(matching_rows) > 0:
        current_group_start = matching_rows[0]
        current_group_end = matching_rows[0]

        for i in range(1, len(matching_rows)):
            row = matching_rows[i]
            if row == current_group_end + 1:
                current_group_end = row
            else:
                lines.append((current_group_start, current_group_end))
                current_group_start = row
                current_group_end = row
        lines.append((current_group_start, current_group_end))

    if len(lines) >= 2:
        y1 = lines[0][0]
        y2 = lines[1][0]
        return y2 - y1 + 1
    
    return 0


def perform_ocr(cleaned_crop, templates, region_name="unknown"):
    """Identifies digits in a cleaned crop using 1:1 template matching."""
    if not templates:
        return ""
    
    print(f"\n  === OCR for {region_name} ===")

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
        candidates_dir = os.path.join("research", "output", "digit_candidates")
        os.makedirs(candidates_dir, exist_ok=True)
        cv2.imwrite(os.path.join(candidates_dir, f"{region_name}_blob{blob_idx}.png"), digit_blob)
        
        # DEBUG: Find Top 3 Leaderboard
        all_matches = []
        for char, template in templates.items():
            t_h, t_w = template.shape
            max_h, max_w = max(t_h, h_c), max(t_w, w_c)
            t_pad = np.zeros((max_h, max_w), dtype=np.uint8)
            b_pad = np.zeros((max_h, max_w), dtype=np.uint8)
            t_pad[(max_h-t_h)//2 : (max_h-t_h)//2 + t_h, (max_w-t_w)//2 : (max_w-t_w)//2 + t_w] = template
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

    print("\n--- OCR Results (Baseline) ---")
    print(json.dumps(results, indent=2))

    output_path = os.path.join("research", "output", "debug_" + os.path.basename(image_path))
    cv2.imwrite(output_path, img)
    print(f"  Saved debug output to {output_path}")
    
    return results

def analyze_screenshot(image_path, config, templates):
    return analyze_screenshot_return_results(image_path, config, templates)

def analyze_screenshot_return_results(image_path, config, templates):
    print(f"Analyzing: {image_path}")
    img = cv2.imread(image_path)
    if img is None:
        print(f"Failed to load {image_path}")
        return

    h, w, _ = img.shape
    print(f"  Resolution: {w}x{h}")
    
    elements = config.get('elements', {})
    
    # 1. Setup Directories
    base_out = os.path.join("research", "output")
    dirs = {
        "raw": os.path.join(base_out, "crops", "raw"),
        "filtered": os.path.join(base_out, "crops", "filtered"),
        "clean": os.path.join(base_out, "crops", "clean"),
        "candidates": os.path.join(base_out, "digit_candidates")
    }
    for d in dirs.values():
        os.makedirs(d, exist_ok=True)

    results = {}
    debug_drawings = [] 

    for name, data in elements.items():
        try:
            x, y, width, height = data['x_px'], data['y_px'], data['w_px'], data['h_px']
            split_pct = data.get('split_pct', 1.0)
            
            debug_drawings.append(('rect', (x, y), (x + width, y + height), (0, 0, 255), 2))
            
            regions = []
            if split_pct < 1.0:
                split_x = int(width * split_pct)
                # Vils (70% size, bottom-right aligned)
                v_rw = int(split_x * 0.70)
                v_rh = int(height * 0.35)
                v_rx = x + (split_x - v_rw) - 2
                v_ry = y + (height - v_rh) - 2
                regions.append(("vils", v_rx, v_ry, v_rw, v_rh))
                
                # Totals (12px vertical padding)
                t_rx = x + split_x + 5
                t_ry = y + 12
                t_rw = width - split_x - 10
                t_rh = height - 24
                regions.append(("total", t_rx, t_ry, t_rw, t_rh))
                
                debug_drawings.append(('line', (x + split_x, y), (x + split_x, y + height), (255, 0, 0), 1))
            else:
                regions.append(("value", x+5, y+5, width-10, height-10))

            results[name] = {}
            for sub_name, rx, ry, rw, rh in regions:
                current_label = f"{name}_{sub_name}"
                
                # --- STAGE 1: RAW CROP ---
                rw = max(1, rw)
                rh = max(1, rh)
                crop = img[ry:ry+rh, rx:rx+rw].copy()
                cv2.imwrite(os.path.join(dirs["raw"], f"{current_label}.png"), crop)
                
                # --- STAGE 2: FILTERED (Grayscale Logic) ---
                filtered = process_stage_2_filter(crop)
                cv2.imwrite(os.path.join(dirs["filtered"], f"{current_label}.png"), filtered)
                
                # --- STAGE 3: CLEAN (Binary Threshold) ---
                cleaned = process_stage_3_clean(filtered)
                cv2.imwrite(os.path.join(dirs["clean"], f"{current_label}.png"), cleaned)
                
                # --- FINAL: OCR ---
                text = perform_ocr(cleaned, templates, current_label)
                results[name][sub_name] = text
                
                debug_drawings.append(('rect', (rx, ry), (rx + rw, ry + rh), (0, 255, 255), 1))
                debug_drawings.append(('text', current_label, (rx, ry - 2)))

        except KeyError as e:
            print(f"    Missing pixel key {e} for element {name}")

    # Draw Debug Visuals
    for cmd in debug_drawings:
        if cmd[0] == 'rect':
            cv2.rectangle(img, cmd[1], cmd[2], cmd[3], cmd[4])
        elif cmd[0] == 'line':
            cv2.line(img, cmd[1], cmd[2], cmd[3], cmd[4])
        elif cmd[0] == 'text':
            cv2.putText(img, cmd[1], cmd[2], cv2.FONT_HERSHEY_SIMPLEX, 0.3, (0, 255, 255), 1)

    print("\n--- OCR Results (Baseline) ---")
    print(json.dumps(results, indent=2))

    output_path = os.path.join("research", "output", "debug_" + os.path.basename(image_path))
    cv2.imwrite(output_path, img)
    print(f"  Saved debug output to {output_path}")
    
    return results

def main():
    test_bench_dir = "test_bench"
    if not os.path.exists(test_bench_dir):
        print("Test bench directory not found!")
        return

    config = load_config() 
    templates = load_templates(os.path.join("research", "templates"))

    for filename in os.listdir(test_bench_dir):
        if filename.lower() == "aoe2_16x9.png":
            analyze_screenshot(os.path.join(test_bench_dir, filename), config, templates)

if __name__ == "__main__":
    main()
