# Decision Log

## Vision Pipeline (Jan 24, 2026)

### 1. OCR Matching Strategy
*   **Decision**: Adopted **1:1 Pixel Overlap (Intersection over Union)** instead of `cv2.matchTemplate` or Tesseract.
*   **Reasoning**:
    *   The game font is small (pixel art).
    *   Standard scaling/resizing destroys the pixel structure, leading to misrecognition.
    *   Tesseract handles low-resolution game text very poorly.
    *   Simple pixel-to-pixel overlap against a perfect template library proved 100% effective once the image was cleaned.

### 2. Preprocessing & Color Filtering
*   **Decision**: Filter pixels based on **Grayscale Consistency** (R≈G≈B) rather than targeting specific colors (e.g., "yellow").
*   **Logic**:
    *   Calculate `max_diff = max(|r-g|, |g-b|, |r-b|)`.
    *   We found `COLOR_TOLERANCE = 40` to be optimal.
    *   This generalizes better than "remove yellow" because it handles any colored background (icons, wood borders, stone borders) while preserving the white text.
*   **Critical Fix**: Must cast image channels to `int16` before subtraction. `uint8` subtraction wraps around (e.g. 5 - 10 = 250), which broke the filter logic.

### 3. Binary Thresholding
*   **Decision**: Lowered Binary Threshold to **110**.
*   **Reasoning**:
    *   Original threshold (150-170) cut off faint/anti-aliased edges of digits like '3', making them look like '5'.
    *   Lowering to 110 captures more of the digit body, preserving unique features.

### 4. Crop Regions
*   **Decision**: Expanded Villager Count boxes to 70% width (aligned right).
*   **Reasoning**: Allows capturing larger numbers (e.g., 3 digits) while still avoiding the UI frames.