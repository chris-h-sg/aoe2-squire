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

4.  **OCR Matching**:
    - Uses **1:1 Pixel Overlap (Intersection over Union)** instead of standard template matching.
    - This is more robust for tiny, low-resolution pixel-art fonts where scaling/resizing introduces blurring.
    - Templates are stored in `research/templates/`.

## Configuration Tweakables

In `poc_vision.py`:
```python
COLOR_TOLERANCE = 40   # Higher = more permissive (keeps more pixels)
BINARY_THRESHOLD = 110 # Lower = captures dimmer pixels
```
