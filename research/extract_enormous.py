
import cv2
import numpy as np
import os

def process_and_extract(image_path, boxes, output_dir):
    img = cv2.imread(image_path)
    if img is None:
        print(f"Failed to load {image_path}")
        return

    os.makedirs(output_dir, exist_ok=True)
    
    blob_count = 0
    for i, (x, y, w, h) in enumerate(boxes):
        print(f"Processing Box {i+1}: ({x}, {y}, {w}, {h})")
        crop = img[y:y+h, x:x+w]
        
        # Convert to grayscale
        gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
        
        # Threshold to find non-black content
        # Using a low threshold to capture everything that isn't almost black
        _, thresh = cv2.threshold(gray, 10, 255, cv2.THRESH_BINARY)
        
        # Save the thresholded crop for debugging
        cv2.imwrite(os.path.join(output_dir, f"box_{i+1}_thresh.png"), thresh)
        
        # Find contours
        contours, _ = cv2.findContours(thresh, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
        
        # Sort contours left-to-right, then top-to-bottom
        # We'll use the bounding box to sort
        rects = [cv2.boundingRect(cnt) for cnt in contours]
        # Only keep blobs that aren't tiny noise
        rects = [r for r in rects if r[2] > 2 and r[3] > 2]
        
        # Sort by y primarily, then x (to handle multiple lines if any)
        # But if it's just one line, x is enough. 
        # Given the large width, they might be horizontal sequences.
        rects.sort(key=lambda r: (r[1] // 50, r[0])) # Group by rows approx 50px high then x
        
        for rx, ry, rw, rh in rects:
            # NEW: Extract from the grayscale image, not the thresholded mask
            # This preserves anti-aliasing for better matching.
            blob_gray = gray[ry:ry+rh, rx:rx+rw]
            
            # Use the thresholded mask to clean the background to pure black
            # (In case there's any faint noise)
            mask_clip = thresh[ry:ry+rh, rx:rx+rw]
            blob_gray[mask_clip == 0] = 0
            
            filename = f"blob_{blob_count:03d}_{rx}_{ry}.png"
            cv2.imwrite(os.path.join(output_dir, filename), blob_gray)
            blob_count += 1

    print(f"Extracted {blob_count} blobs to {output_dir}")

if __name__ == "__main__":
    image_path = r"c:\Users\Chris\sourcetree-repos\rts-analyzer\test_bench\enormous.png"
    boxes = [
        (15, 80, 532, 733),
        (520, 639, 535, 173)
    ]
    output_dir = r"c:\Users\Chris\sourcetree-repos\rts-analyzer\research\output\enormous_blobs"
    process_and_extract(image_path, boxes, output_dir)
