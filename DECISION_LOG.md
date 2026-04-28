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

## Production Tech Stack (Apr 28, 2026)

### 1. Language: Rust for production, Python for R&D only
*   **Decision**: Ship the production app as a native Rust binary. All Python code is R&D/validation only and will not ship.
*   **Rejected alternatives**:
    *   **PyInstaller**: 150–300MB exe, slow startup — effectively still bundling a runtime.
    *   **C# / .NET 8 self-contained**: viable (~60–80MB single-file exe, good OpenCV bindings via OpenCvSharp), but permanently trades away performance headroom and binary size.
*   **Reasoning**:
    *   Truly minimal native binary (~5MB), no runtime for users to manage.
    *   Lowest CPU overhead — TECHNICAL_SPEC.md flags <1% CPU as a hard requirement due to AoE2's single-threaded nature.
    *   `windows-rs` has first-class DXGI support for screen capture.

### 2. No OpenCV in the Rust port
*   **Decision**: Use the `image` + `imageproc` crates instead of OpenCV bindings.
*   **Reasoning**:
    *   All OpenCV operations in the Python POC map directly: trivial ops (threshold, cvtColor, inRange) become raw array ops; connected components, Gaussian blur, and resize have direct `imageproc` equivalents.
    *   The "wiggle SSD" matcher uses `warpAffine` only for ±1px translation shifts — simple array shifts in Rust, no affine math needed.
    *   `opencv-rs` bindings are notoriously painful to set up on Windows.

### 3. No Python non-OpenCV prototype
*   **Decision**: Port directly from Python+OpenCV to Rust+imageproc. No intermediate Python non-OpenCV step.
*   **Reasoning**: The algorithm is proven and stable (all tests pass). A Python non-OpenCV version costs time without de-risking anything specific to the Rust port.

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

## Live Capture Validation (Apr 28, 2026)

### 1. Anchor detection scan area: top 5% of frame
*   **Decision**: Reduced the red-pixel scan area from the top 20% to the top 5% of the frame.
*   **Reasoning**: The 20% crop captured stray red pixels from other applications (taskbar icons, notification badges), causing `detect_ui_scale` to produce a plausible but wrong scale when the game was not the foreground window. Restricting to 5% targets only the HUD strip where the AoE2 anchor actually lives.

### 2. No fallback scale on anchor miss
*   **Decision**: `detect_ui_scale` returns `None` when fewer than 5 red pixels are found. `process_frame` returns `None` immediately; callers log `[no anchor]` and skip the frame.
*   **Rejected**: Silently defaulting to scale `1.0` and processing whatever is on screen.
*   **Reasoning**: A missed anchor means the game is not visible or the UI has changed. Processing with a default scale produces garbage values with no indication anything is wrong.

### 3. Python performance baseline & decision to port
*   **Measured**: 200–300ms per frame on a gaming PC running AoE2:DE (1080p, windowed).
*   **Budget**: 500ms at 2 fps; ~250ms at 4 fps. The pipeline is comfortably within the 2 fps budget.
*   **Decision**: Port to Rust rather than optimise the Python pipeline.
*   **Reasoning**: The bottlenecks (connected components, 9-shift × 11-template SSD matching) are inner loops that will be 10–50× faster in Rust with no special effort. Optimising Python would be throwaway work; the R&D goal was to prove the algorithm, not tune its Python implementation.

## Vision Pipeline (Jan 29, 2026)

### 1. Dynamic Color Handling (Yellow Font & Overlay)
*   **Decision**: Implemented a **Dynamic Mode Switcher** in the base filter that detects and adapts to yellow font/background.
*   **Logic**:
    1.  **Overlay Mode**: If a bright yellow background is detected (`mean(G) > 150, mean(R) > 150`), switch to **Blue Channel Normalization**. This bypasses the yellow noise entirely.
    2.  **Yellow Font Mode**: If no "bright white" pixels exist in a box (indicating colored text), the filter allows "Yellowish" pixels (`R,G > 100, B < max-15`).
    3.  **Luminance Normalization**: Used `max(R, G, B)` for final grayscale conversion.
*   **Reasoning**:
    *   Previously, the population count would disappear when turning yellow (near cap) because it violated the "Grey Tolerance" check.
    *   Housed players needed a way to see digits buried in a bright yellow overlay.
    *   Normalizing colored text to white via the max channel allows 1:1 matching against standard white templates without needing extra colored templates.
