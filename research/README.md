# Research: Vision Pipeline

This directory contains the Proof-of-Concept (PoC) vision system for extracting resource values and villager counts from Age of Empires 2 DE game screenshots.

## Key Scripts

- **`poc_vision.py`**: The main script.
    - **Usage**: `python research/poc_vision.py`
    - **Input**: Reads images from `test_bench/` (expects `aoe2_16x9.png`).
    - **Output**: 
        - Prints extracted values to the console.
        - Saves debug images to `research/output/`:
            - `crops/raw/`: Original extracted regions.
            - `crops/filtered/`: After color-filtering (removing non-grayscale UI elements).
            - `crops/clean/`: After binary thresholding (what the OCR engine sees).
            - `digit_candidates/`: Individual digit blobs found during OCR.
            - `debug_aoe2_16x9.png`: Final annotated debug image.

- **`test_vision.py`**: Unit test suite.
    - **Usage**: `python research/test_vision.py`
    - **Purpose**: Verifies that the extraction logic produces the exact known-good values for the 1080p baseline image. Always run this after making changes to ensure no regressions.

## CLI Usage (poc_vision.py)

You can customize the output and target using the following flags:

| Flag | Description |
| :--- | :--- |
| `--image <filename>` | Run analysis on a specific file in `test_bench` (e.g., `--image aoe2_4k.png`). |
| `--ocr-engine <engine>` | Choose OCR engine: `template` (default) or `tesseract`. |
| `--ocr-raw` | (Tesseract only) Feed raw color crops directly to the OCR engine. |
| `--ocr-filtered` | (Tesseract only) Feed color-filtered crops to OCR without binary thresholding. |
| `--ocr-hybrid` | (Tesseract only) Use filtered crops for Villager counts and raw crops for Resource totals. |
| `--ocr-hints` | (Tesseract only) Use custom word/pattern hints from `research/tess_config/`. |
| `--save-red-mask` | Saves the isolated "Red UI" mask to `research/output/red_mask_<filename>`. Useful for debugging UI scale detection. |
| `--save-debug-image` | Saves the full screenshot with colored bounding boxes drawn on it to `research/output/debug_<filename>`. |
| `--save-candidates` | Extracts every potential digit blob found by OCR into `research/output/digit_candidates/`. **Crucial for gathering new templates.** |
| `--save-crops` | Saves the intermediate processing stages (Raw -> Filtered -> Clean) for every UI element into `research/output/crops/`. |
| `--save-ocr-input` | Saves the exact upscaled (4x) images being passed to Tesseract into `research/output/ocr_input/`. |

### Example: Gathering New Templates
If you find a screenshot where OCR is failing:
1. Place it in `test_bench/`.
2. Run: 
   ```bash
   python research/poc_vision.py --image my_screenshot.png --save-candidates
   ```
3. Inspect `research/output/digit_candidates/`.
4. Rename valid digit images to `{char}.png` (e.g., `5.png`) and move them to `research/templates/`.

## Pipeline Logic

The extraction process follows a 3-stage pipeline:

1.  **Stage 1: Raw Extraction**:
    - Crops regions defined in `ui_map.json`.
    - Handles splitting for Resource/Villager sub-boxes.

2.  **Stage 2: Color Filtering**:
    - **Logic**: Filters out pixels where R, G, B channels differ significantly.
    - **Config**: `COLOR_TOLERANCE` (default: 40).
    - **Purpose**: Removes colored UI backgrounds (like the yellow Idle Villager icon) while keeping the white/grey test.
    - **Critical Implementation Detail**: Must cast channels to `int16` before subtraction to avoid `uint8` overflow.

3.  **Stage 3: Binary Cleanup**:
    - **Logic**: Converts to grayscale and applies a binary threshold.
    - **Config**: `BINARY_THRESHOLD` (default: 110).
    - **Purpose**: Creates a crisp black-and-white image for template matching.

4.  **OCR Matching (Standard)**:
    - Uses **1:1 Pixel Overlap (Intersection over Union)** instead of standard template matching.
    - This is more robust for tiny, low-resolution pixel-art fonts where scaling/resizing introduces blurring.
    - Templates are stored in `research/templates/`.

5.  **Tesseract OCR (Alternative)**:
    - Selectable via `--ocr-engine tesseract`.
    - **Upscaling**: Crops are rescaled by **4x** using cubic interpolation to meet Tesseract's preferred character size.
    - **Preprocessing**:
        - **Edge Smoothing**: Applies a Gaussian blur to rounded off pixelated edges.
        - **Polarity**: Always inverts the image to provide Black-on-White text.
        - **Normalization**: Stretches contrast so the text pops against the background.
    - **Optimization**: Supports **Hybrid Mode** (`--ocr-hybrid`) to use different preprocessing (Raw vs Filtered) depending on the UI element's background color.
    - **Hardcoded Hints**: Uses `research/tess_config/` (words and patterns) to force the engine to prioritize expected RTS resource formats like digit counts and population slashes (`64/75`).

## Tesseract OCR Evaluation

### Findings
- **Small Pixel Fonts**: Tesseract (LSTM engine) struggles with the tiny, low-resolution pixel-art fonts used in the AoE2 HUD. Even with 4x upscaling, recognition is often brittle.
- **Background Noise**: Colored backgrounds (villager icons) introduce noise that confuses the engine, sometimes leading to ghost digits (e.g., `18249` reading as `182493`).
- **Engine Polarity**: Tesseract significantly prefers black-on-white text. We successfully mitigated some issues by normalizing contrast and inverting all inputs.
- **Reliability Conclusion**: While significantly better than standard template matching for scaled/distorted text, it is not yet "100% reliable" without further training.

### Future Improvement Ideas
- **Custom Font Training**: Create a custom `.traineddata` file using synthetic training data generated from the game's actual `.ttf` files (e.g., `Slayer.ttf`). This is the most robust long-term solution.
- **Crop-to-Blobs Optimization**: Prior to OCR, find the bounding box of only the white/colored "blobs" (text) within the crop. This removes background borders/noise from the engine's view.
- **User Dictionary Enforcement**: Strictly enforce that the OCR engine only outputs "words" from our pre-defined dictionary or patterns, rejecting any out-of-scope recognized text.

## Configuration Tweakables

In `poc_vision.py`:
```python
COLOR_TOLERANCE = 40   # Higher = more permissive (keeps more pixels)
BINARY_THRESHOLD = 110 # Lower = captures dimmer pixels
```

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
