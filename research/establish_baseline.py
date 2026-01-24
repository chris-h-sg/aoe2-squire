
import cv2
import os
import numpy as np

def isolate_red(filename, img_path, output_dir):
    img = cv2.imread(img_path)
    if img is None:
        return

    # Create mask for bright red
    # OpenCV uses BGR format
    # Rule: Red > 220, Green < 60, Blue < 60
    
    b, g, r = cv2.split(img)
    
    # Create boolean masks
    mask_r = r > 200
    mask_g = g < 60
    mask_b = b < 60
    
    # Combine masks
    final_mask = mask_r & mask_g & mask_b
    
    # Create black image
    result = np.zeros_like(img)
    
    # Set matched pixels to white
    result[final_mask] = [255, 255, 255]
    
    output_path = os.path.join(output_dir, filename)
    cv2.imwrite(output_path, result)
    # print(f"[{filename}] Saved red isolation to {output_path}")

    # Find rightmost white pixel in top 20% of the image
    h, w, _ = img.shape
    top_h = int(h * 0.2)
    
    # Slice the mask to top 20%
    # final_mask is a 2D boolean array (True/False)
    top_mask = final_mask[0:top_h, :] # Rows 0 to top_h, all columns
    
    # Find coordinates where mask is True (non-zero)
    # np.nonzero returns tuple of arrays (row_indices, col_indices)
    y_idxs, x_idxs = np.nonzero(top_mask)
    
    if len(x_idxs) > 0:
        max_x = np.max(x_idxs)
        dist_from_right = w - max_x
        print(f"[{filename}] Rightmost Red X: {max_x} (Dist from Right: {dist_from_right} px)")
    else:
        print(f"[{filename}] No red pixels found in top 20%")

def run_red_isolation():
    test_bench_dir = "test_bench"
    output_dir = os.path.join("research", "output", "red")
    
    if not os.path.exists(test_bench_dir):
        print(f"Directory not found: {test_bench_dir}")
        return

    os.makedirs(output_dir, exist_ok=True)
    print(f"Processing images from {test_bench_dir} to {output_dir}...")
    
    files = [f for f in os.listdir(test_bench_dir) if f.lower().endswith('.png')]
    files.sort()

    for filename in files:
        image_path = os.path.join(test_bench_dir, filename)
        isolate_red(filename, image_path, output_dir)

if __name__ == "__main__":
    run_red_isolation()
