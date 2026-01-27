import cv2
import numpy as np
import os
import argparse
from pathlib import Path

# ===== CONFIGURATION =====
WORKING_HEIGHT = 36
CANVAS_SIZE = 64
BLUR_SIGMA = 1.0
OFFSETS = [-1, 0, 1]
# ========================

# Global pre-computed translation matrices for Wiggle SSD
WIGGLE_MATRICES = [
    np.float32([[1, 0, dx], [0, 1, dy]])
    for dy in OFFSETS
    for dx in OFFSETS
]

def calculate_h_symmetry(canvas):
    """Calculates horizontal symmetry score (lower is more symmetric)."""
    flipped = cv2.flip(canvas, 1) # 1 = horizontal flip
    diff = canvas - flipped
    return np.sum(diff * diff)

def prepare_canvas(img, target_h):
    """Scales, centers by bounding box, and blurs an image into a fixed canvas."""
    # 1. Upscale
    h_orig, w_orig = img.shape
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

def match_digit(digit_img, processed_templates):
    """Matches a digit image using 'Wiggle SSD' (best of pre-computed offsets)."""
    input_canvas = prepare_canvas(digit_img, WORKING_HEIGHT)
    
    all_matches = []
    
    for char, template_canvas in processed_templates.items():
        best_ssd = float('inf')
        
        # Trial pre-computed wiggle matrices
        for T in WIGGLE_MATRICES:
            # Shift template and calculate SSD
            shifted_template = cv2.warpAffine(template_canvas, T, (CANVAS_SIZE, CANVAS_SIZE))
            
            diff = input_canvas - shifted_template
            ssd = np.sum(diff * diff)
            if ssd < best_ssd:
                best_ssd = ssd
        
        all_matches.append((best_ssd, char))
    
    all_matches.sort()
    
    # --- TIE-BREAKER LOGIC ---
    # In small fonts, '0' is often confused with '3', '6', or '9'.
    # We use horizontal symmetry as a tie-breaker when matches are close.
    tb_info = ""
    if len(all_matches) > 1:
        best_ssd, best_char = all_matches[0]
        second_ssd, second_char = all_matches[1]
        
        asym_chars = {'3', '6', '9'}
        is_conflict = (best_char == '0' and second_char in asym_chars) or \
                      (best_char in asym_chars and second_char == '0')
        
        # Trigger if SSD difference is less than 20% of the winner
        if is_conflict:
            if (second_ssd - best_ssd < best_ssd * 0.20):
                input_canvas = prepare_canvas(digit_img, WORKING_HEIGHT)
                sym_score = calculate_h_symmetry(input_canvas)
                
                # 1. Identified as 0 but is asymmetric (SymH > 60) -> Swap to 3/6/9
                if best_char == '0' and sym_score > 60:
                    all_matches[0], all_matches[1] = all_matches[1], all_matches[0]
                    tb_info = f" [SwapH:{sym_score:.1f}]"
                # 2. Identified as 3/6/9 but is symmetric (SymH < 40) -> Swap to 0
                elif best_char in asym_chars and sym_score < 40:
                    all_matches[0], all_matches[1] = all_matches[1], all_matches[0]
                    tb_info = f" [SwapH:{sym_score:.1f}]"
                else:
                    tb_info = f" [TrustH:{sym_score:.1f}]"
            else:
                tb_info = " [GapOK]"

    return all_matches, tb_info

def parse_expected(filename):
    """Parses the expected value from the filename."""
    if "_val_" in filename:
        val_str = filename.split("_val_")[-1].replace(".png", "")
        if val_str == "slash": return "/"
        if val_str == "dot": return "."
        if val_str == "extra": return "extra"
        return val_str
    return "unknown"

def main():
    parser = argparse.ArgumentParser(description="Match extracted digits against templates.")
    parser.add_argument("--input", type=str, required=True, help="Directory containing extracted digit images")
    parser.add_argument("--templates", type=str, default="research/templates/enormous_numbers", help="Directory containing template images")
    args = parser.parse_args()

    # templates are now pre-processed canvases
    templates = load_templates(args.templates)
    if not templates:
        return

    input_dir = Path(args.input)
    if not input_dir.exists():
        print(f"Input directory {input_dir} not found!")
        return

    files = sorted(list(input_dir.glob("*.png")))
    print(f"Found {len(files)} digits in {input_dir}")
    print(f"{'Filename':<35} | {'Exp':<3} | {'Best':<4} | {'SSD':<7} | {'2nd':<4} | {'SSD2':<7} | {'Status'}")
    print("-" * 90)

    matches = total = 0
    mismatches = []
    closest_margin = float('inf')
    closest_info = ""

    for f in files:
        img = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        if img is None: continue

        expected = parse_expected(f.name)
        all_matches, tb_info = match_digit(img, templates)
        
        best_score, best_char = all_matches[0]
        second_score, second_char = all_matches[1] if len(all_matches) > 1 else (float('inf'), "?")
        
        margin = second_score - best_score
        if margin < closest_margin:
            closest_margin = margin
            closest_info = f"{f.name} (Best: '{best_char}', 2nd: '{second_char}', Margin: {margin:.2f})"

        status = "OK" if best_char == expected else "FAIL"
        status += tb_info
        
        if best_char == expected:
            matches += 1
        elif expected != "extra":
            mismatches.append((f.name, expected, best_char, best_score))
        
        total += 1
        print(f"{f.name[:35]:<35} | {expected:<3} | {best_char:<4} | {best_score:<7.2f} | {second_char:<4} | {second_score:<7.2f} | {status}")

    print("-" * 90)
    if total > 0:
        print(f"Results: {matches}/{total} matched ({100*matches/total:.1f}%)")
        print(f"Closest Second Place: {closest_info}")
    
    if mismatches:
        print("\nMismatches detail:")
        for name, exp, got, score in mismatches:
            print(f"  {name}: Expected '{exp}', Got '{got}' (SSD: {score:0.3f})")

if __name__ == "__main__":
    main()
