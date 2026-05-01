# Research: Vision Pipeline

This directory contains the Proof-of-Concept (PoC) vision system for extracting resource values and villager counts from Age of Empires 2 DE game screenshots.

> **Note:** The Python PoC described here has been successfully ported to a native Rust application (located at the repository root). The `research/` directory remains for reference and algorithm validation purposes only.

## Key Scripts

- **`extractor_enormous.py`**: Specialized extractor for high-resolution images with the "Enormous" HUD setting.
    - **Logic**: Uses a rigid layout and simple thresholding.
    - **Purpose**: Fast extraction and template generation for OCR training.
- **`extractor_regular.py`**: Robust extractor for standard/scaled UI resolutions.
    - **Logic**: Uses dynamic scale detection and **Isolation Masking** recursive segmentation.
    - **Purpose**: High-quality character extraction from complex backgrounds.
- **`poc_video.py`**: Video analysis tool for time-series data extraction.
    - **Logic**: Samples frames at intervals and uses the unified `poc_vision.process_frame()` for consistency.
    - **Purpose**: Batch processing game footage for macro-analysis with 100% logic equivalence to single-image extraction.
- **`matcher.py`**: Robust digit matcher performing evaluation of extracted digits.
    - **Logic**: Uses the **Wiggle SSD** algorithm (Centering + Blurring + 9-Trial Offset).
    - **Purpose**: Verified at 100% accuracy across all UI scales.
- **`test_video.py`**: Automated test suite for validating video extraction consistency.

### Vision Pipeline & OCR

- **`poc_vision.py`**: The main script and core engine.
    - **Logic**: Integrates `extractor_regular` (segmentation) and `matcher` (Wiggle SSD + Symmetry) into a unified pipeline. Features a reusable `process_frame()` function.
    - **Usage**: `python research/poc_vision.py`
    - **Output**: 
        - Prints extracted values to the console with detailed timing.
        - Returns a structured dictionary matching `expected_values.json`.

- **`test_vision.py`**: Unit test suite.
    - **Usage**: `python research/test_vision.py`
    - **Purpose**: Verifies the pipeline against known-good values defined in `test_bench/expected_values.json`.
    - **Coverage**: Parameterized to run on Standard (100%), Min (75%), and Max (125%) UI scale and at different resolutions.

## CLI Usage (poc_vision.py)

You can run the script on any image.

```bash
python research/poc_vision.py [--image <path_to_image>]
```

| Argument | Description |
| :--- | :--- |
| `--image` | Optional. Path to the input image file (e.g., `test_bench/my_screenshot.png`). Defaults to `test_bench/aoe2_16x9.png` if omitted. |

The script will:
1.  Detect the UI scale.
2.  Extract all resource values and villager counts.
3.  Match digits using the pre-warped Wiggle SSD algorithm.
4.  Print the results and detailed timing metrics to the console.

## Pipeline Logic

The extraction process follows an optimized 4-stage pipeline:

1.  **Stage 1: Scale Detection & Cropping**:
    - **Logic**: Slices the top 20% of the image, finds the rightmost red pixel, and calculates `ui_scale` against a baseline.
    - **Purpose**: Ensures coordinates from `ui_map.json` are perfectly adapted to any resolution (720p - 4k).

2.  **Stage 2: Dynamic Color Handling & Segmentation (`extract_digits`)**:
    - **Housed Overlay Background**:
        - Detects if the box has a full-box bright yellow background (`mean(G) > 150`, `mean(R) > 150`, `mean(B) < 100`).
        - **Background Subtraction**: Automatically samples the background color (at pixel 2,2) and subtracts it from the entire box to "undo" the overlay.
        - **Normalization**: Converts the subtracted image to grayscale and normalizes it to full brightness (`[0, 255]`) for accurate matching.
    - **Yellow Font (Dynamic Detection)**:
        - Detects when digits themselves turn yellow (e.g. at/near population cap).
        - **Mode Switch**: If no bright white pixels are present in a box, the filter automatically allows "Yellowish" pixels (`R>100, G>100, B < max-15`).
        - **Max-Channel Grayscale**: Instead of standard conversion, the system uses `max(R,G,B)` for the final grayscale image. This ensures bright yellow digits become pure white, matching standard templates perfectly.
    - **Idle Villager Short-Circuit**: 
        - Before full extraction, `idle_vils` box is checked for **Yellow Pixels** (`contains_yellow`).
        - If no yellow is found (grey icon), the value is immediately returned as "0".
        - This provides a **100-400x speedup** for this common case and prevents mismatched noise.
    - **Final Cleanup**: Applies a soft filter (`OUT_BRIGHTNESS_THRESHOLD=5`) to remove background noise while preserving anti-aliased text edges.
        - **Brightness Filter**: Discards any detected component that does not contain at least one pixel above `SEG_REQUIRED_BRIGHTNESS` (230). This prevents dim background artifacts from being identified as digits.
        - **Split Path**: If >1 blobs are found, recurse into each masked ROI.
        - **Shrink Path**: If 1 blob is found but it's smaller than the ROI, recurse to see if the tighter crop reveals a cleaner split.
        - **Stall Path**: If 1 blob found is same size as ROI and too wide (`w + 2 > h`), it tightens thresholds (**brightness_thresh**, then **grey_tol**, then **connectivity=4**) to force a split.

3.  **Stage 3: Canvas Preparation (`prepare_canvas`)**:
    - **Logic**: Each extracted digit is:
        - Upscaled to a fixed `WORKING_HEIGHT` (36px).
        - Centered in a 64x64 canvas based on its bounding box.
        - Blurred (`sigma=1.0`) to standardize pixel intensity distribution.
    - **Purpose**: Creates a normalized input vector for the matcher.

4.  **Stage 4: Optimized Matching (Pre-Warped Wiggle SSD + Symmetry)**:
    - **Logic**: 
        - **Pre-Warping**: Inverts the search logic by warping the *input digit* 9 times (3x3 grid) instead of warping every template.
        - **SSD**: Calculates Sum of Squared Differences against 11 pre-loaded templates.
        - **Symmetry Tie-Breaker**: If '0' and '3/6/9' are within 20% SSD margin, compares horizontal symmetry scores to resolve the ambiguity.
    - **Performance**: Capable of processing a full UI panel in ~100ms.



## Specialized Digit Extraction (Iterative Segmentation)

We successfully implemented a robust extraction pipeline in `extractor_regular.py` that handles the difficulties of small, antialiased, and touching digits in standard UI scales.

### 1. Dynamic Scale Detection
The system locates the rightmost "Red UI" pixel at the top of the HUD to determine the exact `ui_scale`. This allows us to map baseline coordinates from `ui_map.json` to any resolution.

### 2. Dual-Threshold Logic
A single threshold is insufficient for low-resolution images. We implemented a dual-pass approach:
*   **Segmentation Mask**: Uses a strict filter (High brightness 100, strict gray tolerance) to find clear gaps between characters.
*   **Soft Output**: Uses a lean filter (Low brightness 5, loose gray tolerance) to preserve the original antialiasing and edge detail, which is critical for future OCR accuracy.
### 3. Iterative Refinement
When digits touch (visually merging into one blob), the system detects that a component's `width + 2 > height` and enters a multi-stage recursive refinement loop:
1.  **Increase Brightness Threshold**: The system first attempts to "erode" connections by incrementally increasing the brightness threshold (up to 160). This is prioritised as it more effectively separates digits that share anti-aliasing bleeds.
2.  **Reduce Grey Tolerance**: If brightness adjustment fails, it tightens the grayscale tolerance (down to 2) to identify sharp color discontinuities.
3.  **Ignore Diagonal Connectivity**: If the digits remain fused, it switches from 8-connectivity to **4-connectivity**. This ignores diagonal neighbors, successfully splitting characters that only touch at corners.
4.  **Boundary Transfer**: The boundaries found by this strict "seed" search are applied to the "soft" output image, preserving high-quality edge detail.

## Extraction Performance Conclusions

We benchmarked the pipeline to identify bottlenecks for real-time application:

| Metric | Measurement (Regular Extractor) |
| :--- | :--- |
| **Total Pipeline Time** | ~125ms (8 FPS) |
| **Core Processing Logic** | **~15ms to 19ms** |
| **I/O Overhead (Read/Write)** | **~105ms (85% of total time)** |

**Key Findings:**
*   **Disk Bottleneck**: Nearly 85% of execution time is spent writing ~25 individual PNG files for debugging and template collection.
*   **Real-time Readiness**: The core OpenCV/NumPy logic is extremely fast (under 20ms). 
*   **Future Path**: Moving to a fully in-memory pipeline (Screen Capture -> In-memory processing -> OCR) will remove the I/O bottleneck, allowing for **50+ FPS** performance on modern hardware.

## Configuration Tweakables

In `extractor_regular.py`:
```python
SEG_GREY_TOLERANCE = 20     # Starting tolerance for finding gaps
OUT_GREY_TOLERANCE = 20     # Tolerance for the final visual output
SEG_BRIGHTNESS_THRESHOLD = 100 # Minimum brightness for segmentation seeds
OUT_BRIGHTNESS_THRESHOLD = 5   # Minimum brightness for final output detail
SEG_REQUIRED_BRIGHTNESS = 230  # Discard segments without at least one 230+ pixel
```

## Video Analysis (poc_video.py)

The video POC allows you to extract trends over time from recorded matches.

### Usage
```bash
python research/poc_video.py --video <path> --interval 1.0 --output results.csv [--save-frames]
```

- **`--interval`**: Sampling frequency in seconds (default `1.0`).
- **`--save-frames`**: Dumps the processed frames to `output/frames/` for verification.
- **`--output`**: CSV filename (saved in `research/output/`).
- **`test_video.py`**: Parameterized test suite for validating video extraction.
    - **Usage**: `python research/test_video.py`
    - **Logic**: Iterates through multiple video files and intervals, comparing results against expected CSVs.
    - **Expected File Pattern**: `<video_name>_expected_<interval>s.csv` (e.g., `extract2_expected_1s.csv`).
    - **Status**: Currently identifies several edge-case mismatches (segmentation/matching) being used for further refinement.

## Debugging Features

Both `extractor_regular.py` and `poc_video.py` (via sampling) support a **Debug Mode** to visualize the extraction process.

### Usage
```bash
python research/extractor_regular.py <image_path> [--debug] [--bright-thresh 230]
```

| Argument | Description |
| :--- | :--- |
| `image_path` | Required. Path to the screenshot. |
| `--debug` | Optional. Generates a debug visualization and saves crops to `output/`. |
| `--bright-thresh`| Optional. The minimum brightness a segment must contain (default 230). |

### Debug Artifacts (`output/extractions/<name>/`)
- **`_debug.png`**: The original frame with green bounding boxes drawn around every detected UI element.
- **`boxes/`**: Individual crops of each UI element (Wood, Food, etc.) before segmentation.
- **`digits/`**: The final surgically separated digit images sent to the matcher.

## UI Calibration

If the game's UI layout changes or you need to adjust the bounding boxes for resources, use `research/generate_ui_map.py`.

### Usage
```bash
python research/generate_ui_map.py
```

### Output
*   Updates `ui_map.json` with new coordinates.
*   This file is the "source of truth" for the location of the wood, food, gold, stone, and population panels.


## Font Research & Extraction

We investigated the game's internal font files (`fonts/`) to obtain perfect OCR templates.

### Technology & Format
*   **Format**: The game uses a custom **Texture Atlas** system. Binary `.box` files serve as maps, providing UV coordinates for character glyphs stored in 2048x2048 `.png` / `.dds` texture pages.
*   **Rendering**: Characters are stored as **Multi-channel Signed Distance Fields (MSDF)**. This technique encodes the distance to edges across the Red, Green, and Blue channels, allowing the game to render perfectly sharp text at any resolution (from 720p to 4K).
*   **Decoding**: Extraction requires calculating the `median(R, G, B)` of the distance field. We implemented a custom decoder in `research/font_extractor.py` that includes **Supersampling** (high-res decoding followed by Lanczos downscaling) to produce clean, anti-aliased templates at 11px and 12px sizes.

### Investigated Fonts
*   `combined`: The standard internal serif font.
*   `combined_sansserif`: Used for tooltips and secondary UI elements.
*   `georgia_european`: Used for decorative headings.

### Conclusion: Missing HUD Font
Despite successful extraction of thousands of glyphs, we have determined that **the specific font used for the primary resource counts (Food, Wood, etc.) is likely not in these files.**

**Key Discrepancy:**
*   In-game measurement: The digit '9' is **9px wide** at an **11px height**.
*   Extracted `combined` font: The digit '9' is significantly thinner (**7px wide** at 11px height).
*   The special wide-mapping IDs (0-9) in these files appear to be placeholder boxes or unrelated symbols, not the "High Readability" digits seen in the HUD.

**Current Verdict:** We should continue using screenshot-based template gathering for the main resource panel, as the extracted game fonts do not match the HUD's specific aspect ratios.

## Digit Matching Research (Wiggle SSD)

We achieved **100% recognition accuracy** across all supported UI scales through the following research iterations:

### 1. Robust Centering & Blurring
Standard template matching fails when digits are shifted by even a half-pixel due to game engine sub-pixel snapping. We solved this by:
- Upscaling digits to 36px to preserve gradient information.
- Using the **Geometric Center** of the character's bounding box for initial alignment.
- Applying a **Gaussian Blur** (sigma=1.0). This "spreads" the pixel intensities, allowing the SSD to overlap meaningfully even if edges don't align perfectly.

### 2. The "Wiggle" Offset Trial
To overcome vertical jitter and horizontal snapping issues, we trial **9 local offsets** ([-1, 0, 1] pixels in X and Y). 
- The matcher calculates the SSD for all 9 positions.
- The **Minimum SSD** is taken as the final score.
- This approach effectively "finds" the best fit, making the system immune to the jitter found in low-res screenshots.

### 3. Discriminative Power & Symmetry Tie-Breaker
Because '0' can look extremely similar to '3', '6', and '9' in small fonts, we've implemented a **Horizontal Symmetry Tie-Breaker**:
- **Mechanism**: The digit is flipped horizontally and compared to its original state.
- **Trigger**: Only activates if the SSD margin between a '0' and any of {'3', '6', '9'} is **< 20%**.
- **Rule**:
    - If '0' wins but Symmetry is high (Asymmetric > 60), it's overridden to the second-place char.
    - If an asymmetric char wins but Symmetry is low (Symmetric < 40), it's overridden to '0'.
- This resolved the final remaining misidentifications in low-res images.

### Evaluation Results

| UI Scale | Samples | Accuracy | Closest Margin | Tie-Breaker Applied |
| :--- | :--- | :--- | :--- | :--- |
| **Max** (16x9_max) | 33 | 100% | 18.43 (7 vs /) | No |
| **Default** (16x9) | 33 | 100% | 25.73 (5 vs 3) | No |
| **Min** (16x9_min) | 33 | 100% | -13.46 (9 vs 0) | Yes (stone_vils) |
