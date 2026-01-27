"""
Extractor Pipeline - Structural processing of UI images
"""

import cv2
import numpy as np
import json
import os
import argparse
import shutil
from pathlib import Path

# Red pixel detection config (BGR)
RED_MASK_LOWER = np.array([0, 0, 201])
RED_MASK_UPPER = np.array([60, 60, 255])

def detect_ui_scale(img, baseline_margin=263):
    """Detect UI scale by finding rightmost red pixel."""
    h, w, _ = img.shape
    top_h = int(h * 0.2)
    crop = img[0:top_h, :]
    
    mask = cv2.inRange(crop, RED_MASK_LOWER, RED_MASK_UPPER)
    y_idxs, x_idxs = np.nonzero(mask)
    
    if len(x_idxs) < 5:
        return 1.0
    
    max_x = np.max(x_idxs)
    current_margin = w - max_x
    return current_margin / baseline_margin

def step1_extract_boxes(img, is_enormous, ui_map=None):
    """
    Step 1: Extract raw boxes from image.
    Returns: List of dicts {'name': str, 'image': ndarray}
    """
    boxes = []
    
    if is_enormous:
        # Step 1.b: Hardcoded box + row splitting
        x_offset, y_offset = 15, 80
        width, height = 530, 920
        h_img, w_img = img.shape[:2]
        x_end = min(x_offset + width, w_img)
        y_end = min(y_offset + height, h_img)
        
        main_box = img[y_offset:y_end, x_offset:x_end]
        
        # Split into rows
        gray = cv2.cvtColor(main_box, cv2.COLOR_BGR2GRAY) if len(main_box.shape) == 3 else main_box
        _, binary = cv2.threshold(gray, 1, 255, cv2.THRESH_BINARY)
        row_projection = np.sum(binary, axis=1)
        
        in_row = False
        row_start = 0
        
        # Consistent names for enormous rows (top to bottom)
        enormous_names = ['wood_total', 'food_total', 'gold_total', 'stone_total', 'population_total']
        row_idx = 0
        
        for y in range(len(row_projection)):
            if row_projection[y] > 0 and not in_row:
                row_start = y
                in_row = True
            elif row_projection[y] == 0 and in_row:
                if y - row_start >= 100:
                    name = enormous_names[row_idx] if row_idx < len(enormous_names) else f"extra_row_{row_idx}"
                    row_img = main_box[row_start:y, :]
                    boxes.append({'name': name, 'image': row_img})
                    row_idx += 1
                in_row = False
        if in_row:
            if len(row_projection) - row_start >= 100:
                name = enormous_names[row_idx] if row_idx < len(enormous_names) else f"extra_row_{row_idx}"
                row_img = main_box[row_start:len(row_projection), :]
                boxes.append({'name': name, 'image': row_img})
            
    else:
        # Step 1.a: Scaled coordinates from UI map
        if ui_map is None:
            return []
            
        baseline_margin = ui_map.get('baseline_margin', 263)
        ui_scale = detect_ui_scale(img, baseline_margin)
        
        elements = ui_map.get('elements', {})
        for name, coords in elements.items():
            x = int(coords['x_px'] * ui_scale)
            y = int(coords['y_px'] * ui_scale)
            w = max(1, int(coords['w_px'] * ui_scale))
            h = max(1, int(coords['h_px'] * ui_scale))
            
            box_img = img[y:y+h, x:x+w].copy()
            boxes.append({'name': name, 'image': box_img})
            
    return boxes

def step2_cleanup_box(box_img):
    """
    Step 2: Cleanup.
    - Converts to grayscale.
    - Zeroes out pixels with brightness < 20.
    """
    if len(box_img.shape) == 3:
        box_img = cv2.cvtColor(box_img, cv2.COLOR_BGR2GRAY)
        
    # Zero out pixels with brightness < 20 (reduces background noise)
    _, cleaned = cv2.threshold(box_img, 20, 255, cv2.THRESH_TOZERO)
    
    return cleaned

def step3_segment_into_digits(box_img, min_digit_area=100):
    """
    Step 3: Segmentation into digits.
    Identifies digits as contiguous areas (connected components) with area > 100.
    """
    # Ensure binary for component analysis
    _, binary = cv2.threshold(box_img, 1, 255, cv2.THRESH_BINARY)
    
    num_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(binary, connectivity=8)
    
    components = []
    for i in range(1, num_labels): # Skip background
        area = stats[i, cv2.CC_STAT_AREA]
        if area >= min_digit_area:
            x = stats[i, cv2.CC_STAT_LEFT]
            y = stats[i, cv2.CC_STAT_TOP]
            w = stats[i, cv2.CC_STAT_WIDTH]
            h = stats[i, cv2.CC_STAT_HEIGHT]
            
            # Extract the component from the original box_img (preserving grayscale/cleanup)
            comp_img = box_img[y:y+h, x:x+w].copy()
            # Mask out other components in the same bounding box
            mask = (labels[y:y+h, x:x+w] == i).astype(np.uint8) * 255
            comp_img = cv2.bitwise_and(comp_img, mask)
            
            components.append({
                'x': x,
                'image': comp_img
            })
            
    # Sort components by X position (left to right)
    components.sort(key=lambda c: c['x'])
    
    return [c['image'] for c in components]

def save_to_templates(digits_info, templates_dir):
    """
    Saves the first instance of each unique character as a template.
    digits_info: List of dicts {'char': str, 'image': ndarray}
    """
    os.makedirs(templates_dir, exist_ok=True)
    saved_chars = set()
    
    for digit in digits_info:
        char = digit['char']
        if char not in saved_chars and char != "extra":
            # Sanitize character for filename
            char_name = char
            if char == "/": char_name = "slash"
            elif char == ".": char_name = "dot"
            
            template_path = os.path.join(templates_dir, f"{char_name}.png")
            cv2.imwrite(template_path, digit['image'])
            saved_chars.add(char)
            print(f"Saved template: {template_path}")

def run_pipeline(image_path, is_enormous, ui_map=None):
    img = cv2.imread(image_path)
    if img is None:
        print(f"Error: Could not load {image_path}")
        return
        
    image_name = Path(image_path).stem
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_base = os.path.join(script_dir, 'output', 'extractions', image_name)
    
    if os.path.exists(output_base):
        try:
            shutil.rmtree(output_base)
        except PermissionError:
            print(f"Warning: Permission denied when cleaning {output_base}. Some files may be in use.")
            
    os.makedirs(output_base, exist_ok=True)
    
    # --- Step 1: Extraction ---
    print(f"Step 1: Extracting boxes from {image_name}...")
    boxes = step1_extract_boxes(img, is_enormous, ui_map)
    
    # Save boxes from Step 1 for inspection
    step1_dir = os.path.join(output_base, "step1_boxes")
    os.makedirs(step1_dir, exist_ok=True)
    for box in boxes:
        cv2.imwrite(os.path.join(step1_dir, f"{box['name']}.png"), box['image'])
    
    # --- Load Expected Values for Naming ---
    expected_values = {}
    expected_path = os.path.join(script_dir, '..', 'test_bench', 'expected_values.json')
    if os.path.exists(expected_path):
        with open(expected_path, 'r') as f:
            data = json.load(f)
            img_filename = f"{image_name}.png"
            if img_filename in data.get('images', {}):
                val_set_name = data['images'][img_filename].get('use_value_set')
                if val_set_name in data.get('value_sets', {}):
                    expected_values = data['value_sets'][val_set_name]

    # --- Step 2 & 3: Cleanup and Segmentation ---
    digit_dir = os.path.join(output_base, "digits")
    os.makedirs(digit_dir, exist_ok=True)
    
    collected_digits = []
    
    for box in boxes:
        print(f"Processing box: {box['name']}...")
        
        # Step 2: Cleanup
        cleaned = step2_cleanup_box(box['image'])
        
        # Step 3: Segmentation
        digits = step3_segment_into_digits(cleaned)
        
        # Get expected string for this box
        resource_type = box['name'].split('_')[0]
        sub_type = box['name'].split('_')[1] if '_' in box['name'] else 'value'
        
        target_str = ""
        if resource_type in expected_values:
            target_str = expected_values[resource_type].get(sub_type, "")
        
        # Save digits with naming: pos_N_val_CHAR.png
        for i, digit_img in enumerate(digits):
            char = target_str[i] if i < len(target_str) else "extra"
            # Sanitize character for filename
            char_name = char
            if char == "/": char_name = "slash"
            elif char == ".": char_name = "dot"
            
            filename = f"{box['name']}_pos_{i:02d}_val_{char_name}.png"
            cv2.imwrite(os.path.join(digit_dir, filename), digit_img)
            
            collected_digits.append({'char': char, 'image': digit_img})
    
    # --- Final Step: Template Collection (Enormous only) ---
    if is_enormous:
        print("Collecting templates for enormous numbers...")
        templates_dir = os.path.join(script_dir, "templates", "enormous_numbers")
        save_to_templates(collected_digits, templates_dir)
    
    print(f"Pipeline complete. Digits saved in {digit_dir}")

def main():
    parser = argparse.ArgumentParser(description="Image Processing Pipeline")
    parser.add_argument("image_path", help="Path to the image")
    parser.add_argument("--enormous", action="store_true", help="Process as enormous image")
    args = parser.parse_args()
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    ui_map_path = os.path.join(script_dir, '..', 'ui_map.json')
    ui_map = None
    if os.path.exists(ui_map_path):
        with open(ui_map_path, 'r') as f:
            ui_map = json.load(f)
            
    run_pipeline(args.image_path, args.enormous, ui_map)

if __name__ == "__main__":
    main()
