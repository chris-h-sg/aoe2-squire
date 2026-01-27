"""
Extractor Pipeline - Structural processing of enormous UI images
"""

import cv2
import numpy as np
import json
import os
import argparse
import shutil
from pathlib import Path

def step1_extract_boxes(img):
    """
    Step 1: Extract raw boxes from enormous image.
    Returns: List of dicts {'name': str, 'image': ndarray}
    """
    boxes = []
    
    # Hardcoded box + row splitting
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
            
    return boxes

def step2_cleanup_box(box_img):
    """
    Step 2: Cleanup.
    - Converts to grayscale.
    - Zeroes out pixels with brightness < 20.
    """
    if len(box_img.shape) == 3:
        box_img = cv2.cvtColor(box_img, cv2.COLOR_BGR2GRAY)
        
    _, cleaned = cv2.threshold(box_img, 180, 255, cv2.THRESH_TOZERO)
    return cleaned

def step3_segment_into_digits(box_img, min_digit_area=100):
    """
    Step 3: Segmentation into digits for enormous images.
    """
    _, binary = cv2.threshold(box_img, 1, 255, cv2.THRESH_BINARY)
    num_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(binary, connectivity=8)
    
    components = []
    for i in range(1, num_labels):
        area = stats[i, cv2.CC_STAT_AREA]
        if area >= min_digit_area:
            x = stats[i, cv2.CC_STAT_LEFT]
            y = stats[i, cv2.CC_STAT_TOP]
            w = stats[i, cv2.CC_STAT_WIDTH]
            h = stats[i, cv2.CC_STAT_HEIGHT]
            
            comp_img = box_img[y:y+h, x:x+w].copy()
            mask = (labels[y:y+h, x:x+w] == i).astype(np.uint8) * 255
            comp_img = cv2.bitwise_and(comp_img, mask)
            
            components.append({'x': x, 'image': comp_img})
            
    components.sort(key=lambda c: c['x'])
    return [c['image'] for c in components]

def save_to_templates(digits_info, templates_dir):
    os.makedirs(templates_dir, exist_ok=True)
    saved_chars = set()
    for digit in digits_info:
        char = digit['char']
        if char not in saved_chars and char != "extra":
            char_name = char
            if char == "/": char_name = "slash"
            elif char == ".": char_name = "dot"
            
            template_path = os.path.join(templates_dir, f"{char_name}.png")
            cv2.imwrite(template_path, digit['image'])
            saved_chars.add(char)
            print(f"Saved template: {template_path}")

def run_pipeline(image_path):
    img = cv2.imread(image_path)
    if img is None:
        print(f"Error: Could not load {image_path}")
        return
        
    image_name = Path(image_path).stem
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_base = os.path.join(script_dir, 'output', 'extractions', f"{image_name}_enormous")
    
    if os.path.exists(output_base):
        try:
            shutil.rmtree(output_base)
        except PermissionError:
            print(f"Warning: Permission denied when cleaning {output_base}.")
            
    os.makedirs(output_base, exist_ok=True)
    
    print(f"Extracting boxes from {image_name}...")
    boxes = step1_extract_boxes(img)
    
    # Load expected values
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

    digit_dir = os.path.join(output_base, "digits")
    os.makedirs(digit_dir, exist_ok=True)
    collected_digits = []
    
    for box in boxes:
        print(f"Processing box: {box['name']}...")
        cleaned = step2_cleanup_box(box['image'])
        digits = step3_segment_into_digits(cleaned)
        
        resource_type = box['name'].split('_')[0]
        sub_type = box['name'].split('_')[1] if '_' in box['name'] else 'value'
        target_str = expected_values.get(resource_type, {}).get(sub_type, "")
        
        for i, digit_img in enumerate(digits):
            char = target_str[i] if i < len(target_str) else "extra"
            char_name = "slash" if char == "/" else ("dot" if char == "." else char)
            filename = f"{box['name']}_pos_{i:02d}_val_{char_name}.png"
            cv2.imwrite(os.path.join(digit_dir, filename), digit_img)
            collected_digits.append({'char': char, 'image': digit_img})
    
    print("Collecting templates...")
    templates_dir = os.path.join(script_dir, "templates", "enormous_numbers")
    save_to_templates(collected_digits, templates_dir)
    print(f"Pipeline complete. Digits saved in {digit_dir}")

def main():
    parser = argparse.ArgumentParser(description="Enormous Image Processing Pipeline")
    parser.add_argument("image_path", help="Path to the image")
    args = parser.parse_args()
    run_pipeline(args.image_path)

if __name__ == "__main__":
    main()
