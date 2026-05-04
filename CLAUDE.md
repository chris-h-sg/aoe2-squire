# RTS Analyzer — Project Context

## What We're Building
A native Windows background tool that scrapes AoE2:DE resource/villager counts from the screen in real time and combines them with replay data to produce post-game efficiency coaching. Target: minimal CPU footprint (<1% to avoid impacting the game), no memory hooking, no anti-cheat risk.

## Current Status
**Phase 4 in progress — Validation & Sync Tool.**

- Python R&D (`research/`) is finished and all tests pass. Do not modify it unless fixing a bug that needs to be carried into Rust.
- The Rust production binary is fully functional. The entire vision pipeline is complete and operating under 1% CPU overhead.
- Telemetry CSV logging and a fully-featured Replay Parser (via the `liouh/aoe2rec` fork) are implemented.
- **Current Task:** The "Mesher" and "Idle Analyzer" are fully functional. Phase 4 (Validation & Sync) is complete. Transitioning to Phase 5: State Snapshotting.

## Repo Layout
```
rts-analyzer/
├── CLAUDE.md               ← you are here
├── DECISION_LOG.md         ← all architectural decisions with rationale — read this
├── TECHNICAL_SPEC.md       ← product-level requirements
├── Cargo.toml              ← Rust manifest (image 0.25, imageproc 0.25, windows 0.58, serde 1.0)
├── ui_map.json             ← element coordinates (baseline 1080p, scaled at runtime)
├── src/
│   ├── main.rs             ← entry point: load ui_map.json + templates, run process_frame
│   ├── constants.rs        ← all pipeline constants (mirrors poc_vision.py constants)
│   ├── types.rs            ← UiMap, UiElement, Templates, Results
│   ├── pipeline/
│   │   ├── mod.rs          ← process_frame orchestration
│   │   ├── anchor.rs       ← Stage 1: detect_ui_scale (DONE)
│   │   ├── filter.rs       ← Stage 4: apply_base_filter, cleanup_box, contains_yellow, detect_housed_overlay (DONE)
│   │   ├── segment.rs      ← Stage 5: segment_into_digits (STUB)
│   │   ├── canvas.rs       ← Stage 6: prepare_canvas (STUB)
│   │   └── matcher.rs      ← Stage 7: load_templates, match_digit (STUB)
│   ├── bin/
│   │   ├── mesher.rs       ← align telemetry CSV with .aoe2record ground truth
│   │   └── idle_analyzer.rs ← calculate villager-seconds lost per age
│   └── capture/mod.rs      ← DXGI screen capture
├── test_bench/             ← reference screenshots + expected_values.json
│   ├── aoe2_16x9.png       ← 1080p baseline
│   ├── aoe2_16x9_min.png   ← 75% UI scale
│   ├── aoe2_16x9_max.png   ← 125% UI scale
│   ├── aoe2_16x10.png, aoe2_4k.png, aoe2_21x9.png, aoe2_32x9.png
│   └── expected_values.json
└── research/               ← Python R&D only, do not port code from here blindly
    ├── poc_vision.py       ← canonical pipeline implementation to port
    ├── poc_video.py        ← batch video processor (reference only)
    ├── poc_capture.py      ← live capture validation (reference only)
    ├── templates/enormous_numbers/   ← digit template PNGs (0-9 + slash)
    ├── test_vision.py      ← run from repo root: python research/test_vision.py
    ├── test_video.py       ← run from repo root: python research/test_video.py
    └── test_interpolation.py ← run from repo root: python research/test_interpolation.py
```

## The Pipeline (port this to Rust)

All logic lives in `research/poc_vision.py`. Read it. The stages:

### 1. Anchor Detection & Scale (`detect_ui_scale`)
- Crop top **5%** of frame (not 20% — see DECISION_LOG).
- Mask for red pixels: R > 201, G < 60, B < 60.
- If fewer than 5 red pixels found → return `None` (no fallback, log and skip frame).
- `ui_scale = (frame_width - rightmost_red_x) / 263` (baseline margin = 263px at 1080p).

### 2. Region Cropping
- Load `ui_map.json`. Each element has `x_px, y_px, w_px, h_px` at baseline 1080p.
- Multiply all coords by `ui_scale` to get actual pixel coords.
- Crop the region from the frame.

### 3. Special Cases Before Digit Extraction
- **`idle_vils`**: Check for yellow pixels first (`contains_yellow`). If none → value is `"0"`, skip extraction entirely. Yellow criteria: R>100, G>100, B < min(R,G)−30, |R−G| < 50.
- **`population_total`**: Check for housed overlay (`mean(G)>150, mean(R)>150, mean(B)<100`). If detected → use overlay mode (background subtraction + normalize).

### 4. Color Filtering (`apply_base_filter`)
Standard mode (all fields except overlay):
- Per-pixel: compute `max_channel − min_channel`. Keep pixel if diff ≤ 20 (grey tolerance).
- Apply threshold: zero out pixels below brightness 5.
- Output is single-channel (use `max(R,G,B)` as grey value, so yellow text maps to white).

Overlay mode (`population_total` when housed):
- Sample background color at pixel [2,2].
- Subtract background from entire box (clamp to 0).
- Convert subtracted image to greyscale, normalize 0→255, threshold at 30.

### 5. Digit Segmentation (`step3_segment_into_digits`)
- Run connected components on the filtered image.
- Discard components with area < `15 * ui_scale²` or no pixel ≥ 230 brightness.
- If a component is wider than tall (`w + 2 > h`): it's touching digits — recurse with tighter thresholds (increase brightness threshold by 10 up to 160, then decrease grey tolerance by 2 down to 2, then switch to 4-connectivity).
- Sort surviving bounding boxes left-to-right.
- Crop digit images from the **soft-filtered** output (not the strict segmentation image).

### 6. Canvas Preparation (`prepare_canvas`)
Per digit:
- Scale height to `WORKING_HEIGHT = 36px` (preserve aspect ratio).
- Find bounding box of actual content, center it in a `64×64` canvas.
- Gaussian blur sigma=1.0.
- Convert to float32 [0.0, 1.0].

### 7. Matching (`match_digit_to_template`)
- Pre-shift the input canvas 9 times (±1px in X and Y).
- For each template (0–9 + `/`): compute minimum SSD across the 9 shifts.
- Sort by SSD ascending → best match wins.
- Tie-breaker: if winner is `0` and runner-up is `3/6/9` (or vice versa) and SSD margin < 20%:
  - Compute horizontal symmetry score (`sum((canvas − hflip(canvas))²)`).
  - `0` is symmetric (score < 40); `3/6/9` are asymmetric (score > 60). Swap if contradicted.

### 8. Output
Collect digit strings per element. Return dict structured as `{category: {sub_key: value_str}}` matching the `name.split('_')` convention (e.g. `"wood_total"` → `results["wood"]["total"]`).

## Key Constants (from poc_vision.py)
```
RED_MASK_LOWER      = [0, 0, 201]  (BGR)
RED_MASK_UPPER      = [60, 60, 255]
BASELINE_MARGIN     = 263
OUT_GREY_TOLERANCE  = 20
OUT_BRIGHTNESS_THRESHOLD = 5
SEG_GREY_TOLERANCE  = 30
SEG_BRIGHTNESS_THRESHOLD = 100
SEG_REQUIRED_BRIGHTNESS = 230
BASELINE_MIN_AREA   = 15
WORKING_HEIGHT      = 36
CANVAS_SIZE         = 64
BLUR_SIGMA          = 1.0
WIGGLE_OFFSETS      = [-1, 0, 1]
```

## Rust Decisions (already locked — see DECISION_LOG)
- **Crates**: `image = "0.25"` + `imageproc = "0.25"` for pixel ops. No `opencv-rs` (painful Windows setup).
- **Screen capture**: `windows = "0.58"` with DXGI Desktop Duplication API features.
- **No OpenCV**: every OpenCV call in the Python code maps to a simple array op or has a direct `imageproc` equivalent. The wiggle SSD uses only ±1px array shifts — no `warpAffine` needed.
- **Single binary**, no runtime, target ~5MB exe.
- **Channel ordering**: the `image` crate uses **RGB** (not BGR like OpenCV). All channel-order-sensitive code in `filter.rs` and `anchor.rs` accounts for this — R=index 0, G=1, B=2.

## Validating the Port
Run the Python tests to get ground-truth output for the same inputs:
```
python research/test_vision.py   # static screenshots
python research/test_video.py    # video frame extraction
python research/test_interpolation.py # priority-based state interpolation logic
```
Expected values are in `test_bench/expected_values.json`.
