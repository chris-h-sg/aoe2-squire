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
*   **Reasoning**: Allows capturing larger numbers (e.g., 3 digits) while still avoiding the UI frames.

### 5. UI Scale Detection
*   **Decision**: Determine UI Scale dynamically by measuring the **Distance of the rightmost red UI element from the right edge**.
*   **Method**:
    *   Filter for bright red pixels (>200 Red, <60 Green/Blue).
    *   Find the rightmost pixel in the top 20% of the screen.
    *   **Right Margin** = `ImageWidth - Max_X`.
*   **Baseline**:
    *   At the 1080p baseline (`aoe2_16x9.png`), the margin is **265 pixels**.
*   **Reasoning**:
    *   The top panel borders (lines) varied too much in thickness and color across resolutions/settings.
    *   The "Age/Idle/Menu" cluster in the top right contains consistent bright red pixels that move proportionally with the UI scale.

## OCR Engine: Tesseract Evaluation (Jan 25, 2026)
*   **Decision**: Tesseract is **not reliable enough** for out-of-the-box production use without custom font training.
*   **Reasoning**:
    *   While better than template matching for scaled/distorted text, its LSTM engine is not optimized for pixel-art fonts.
    *   It frequently generates "ghost digits" due to background noise in the crops.
    *   Required preprocessing (4x scaling, blurring, inversion) adds significant complexity and run-time overhead compared to the current 1:1 pixel overlap solution.
*   **Future Path**: Only adopt if we implement **Synthetic Font Training** to create a specialized `.traineddata` file for the game's specific HUD font.

## OCR Infrastructure (Jan 27, 2026)

### 1. Robust Digit Segmentation
*   **Decision**: Implemented a **Multi-Stage Iterative Refinement** in `get_components_recursive`.
*   **Logic**:
    1.  If a blob is wider than it is tall (`w > h`), indicating touching digits:
    2.  First, decrease `grey_tol` incrementally (step: 2, min: 2).
    3.  If still touching, increase `brightness_threshold` incrementally (step: 10, max: 160).
*   **Reasoning**: Tightening grey tolerance separates digits that touch via soft anti-aliased shadows, while increasing brightness separates digits that actually "bleed" into each other in low-contrast scenarios.

### 2. Matcher Tie-Breaker: Horizontal Symmetry
*   **Decision**: Introduced a **Horizontal Symmetry Override** for conflicts between '0' and {'3', '6', '9'}.
*   **Parameters**:
    *   **Margin**: SSD difference < 20%.
    *   **Logic**: Flip digit horizontally; calculate `SSD(orig, flipped)`.
    *   **Thresholds**: '0' must be < 40 (Symmetric); '3', '6', '9' must be > 60 (Asymmetric).
*   **Reasoning**: Specifically targets the weakness where small circular fonts make '0' and '9' look identical to a pixel-wise SSD, but preserve their fundamental symmetry differences.

### 3. Expected Value Schema
*   **Decision**: Consolidated all resource lookups to follow the `{main_type}_{sub_type}` pattern.
*   **Fix**: Renamed `idle_vils` to `idle` (main) and `vils` (sub) in `expected_values.json` to match the extractor's parsing logic (`name.split('_')`).
*   **Reasoning**: Prevents special-case hardcoding and ensures the extractor can dynamically look up ground truth for any UI element.
