

import cv2
import os
import sys

# Ensure we can import from the current directory
sys.path.append(os.path.dirname(os.path.abspath(__file__)))

from poc_vision import get_resource_panel_height

def establish_baseline():
    test_bench_dir = "test_bench"
    if not os.path.exists(test_bench_dir):
        print(f"Directory not found: {test_bench_dir}")
        return

    print(f"Scanning images in {test_bench_dir}...")
    
    files = [f for f in os.listdir(test_bench_dir) if f.lower().endswith('.png')]
    files.sort()

    for filename in files:
        image_path = os.path.join(test_bench_dir, filename)
        img = cv2.imread(image_path)
        
        if img is None:
            print(f"[{filename}] Failed to load.")
            continue
            
        height = get_resource_panel_height(img)
        print(f"[{filename}] Detected Panel Height: {height} pixels")

if __name__ == "__main__":
    establish_baseline()

