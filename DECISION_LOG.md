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

## Replay Parsing (Apr 29, 2026)

### 1. Robust Parser Library: liouh Fork
*   **Decision**: Adopted the **liouh/aoe2rec** fork over the original `aoe2rec` crate.
*   **Reasoning**:
    *   The original crate (and other forks) crashed on modern DE replays due to binary misalignment in AI actions and `Sync` operations.
    *   The `liouh` fork implements a sophisticated re-sync loop that correctly handles the DE-specific checksums and varying action lengths.
    *   Successfully processed over 175,000 operations in a single match without a "bad magic" panic.

### 2. ID Mapping Strategy: Community Standard JSON
*   **Decision**: Use a local `aoe2_data.json` generated from the **hszemi/aoe2techtree** project.
*   **Reasoning**:
    *   Mapping thousands of unit/tech/building IDs manually is error-prone and unmaintainable.
    *   Using the `aoe2techtree` data ensures compatibility with the latest DE balance patches and civilization additions (e.g., Romans, Armenians, Georgians).
    *   Using internal engine names (e.g., `VMBAS`, `RTWC`) is more robust than localized display names.

### 3. Timing Logic: Millisecond Accumulation
*   **Decision**: Accumulate time via `Sync` operation increments and verify against the `world_time` field in `Action` packets.
*   **Reasoning**:
    *   Initial confusion regarding the "1.1s Loom" was resolved by ensuring the re-sync loop captures every single `Sync` operation.
    *   Verified that game-time milliseconds are correctly recorded and provide sub-second precision for event tracking.

## Rust Port Scaffold (Apr 28, 2026)

### 1. `image` crate channel ordering
*   The `image` crate loads pixels as **RGB** (index 0=R, 1=G, 2=B), opposite of OpenCV's BGR.
*   All channel-sensitive logic in `filter.rs` and `anchor.rs` uses RGB indices — do not copy Python's `img[:,:,0]` = B assumption into Rust.

## Live Capture Validation (Apr 28, 2026)

### 1. Anchor detection scan area: top 5% of frame
*   **Decision**: Reduced the red-pixel scan area from the top 20% to the top 5% of the frame.
*   **Reasoning**: The 20% crop captured stray red pixels from other applications (taskbar icons, notification badges), causing `detect_ui_scale` to produce a plausible but wrong scale when the game was not the foreground window. Restricting to 5% targets only the HUD strip where the AoE2 anchor actually lives.

### 2. No fallback scale on anchor miss
*   **Decision**: `detect_ui_scale` returns `None` when fewer than 5 red pixels are found. `process_frame` returns `None` immediately; callers log `[no anchor]` and skip the frame.
*   **Rejected**: Silently defaulting to scale `1.0` and processing whatever is on screen.
*   **Reasoning**: A missed anchor means the game is not visible or the UI has changed. Processing with a default scale produces garbage values with no indication anything is wrong.

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

## Data Sources (Apr 30, 2026)

The project relies on several community-maintained resources for AoE2:DE unit, building, and technology metadata:

*   **Unit Statistics**: [https://unitstatistics.com/age-of-empires2/](https://unitstatistics.com/age-of-empires2/) (Primary source for unit attributes)
*   **Object Tables**: [https://airef.github.io/tables/objects.html](https://airef.github.io/tables/objects.html) (Detailed ID mapping for units and buildings)
*   **Technology Tables**: [https://airef.github.io/tables/techs.html](https://airef.github.io/tables/techs.html) (Detailed ID mapping for technologies)
*   **Halfon Reference**: [https://halfon.aoe2.se/](https://halfon.aoe2.se/) (Comprehensive unit, building, and tech data)
*   **Halfon Data (JSON)**: [https://github.com/SiegeEngineers/halfon/blob/master/data/units_buildings_techs.de.json](https://github.com/SiegeEngineers/halfon/blob/master/data/units_buildings_techs.de.json) (The primary source for automated resource cost and ID lookups)

## Technology Metadata (Apr 30, 2026)

### 1. Source: techs.csv vs JSON
*   **Decision**: Switched technology ID mapping from JSON to `data/techs.csv`.
*   **Reasoning**:
    *   `techs.csv` is more human-readable and allows for easier manual verification and updates.
    *   Includes additional metadata like resource costs and ages, which are essential for future "Efficiency Gap" analysis.
    *   Consolidates game data into the central `data/` directory.

### 2. Output: Tabular Research Timeline
*   **Decision**: Display technology research events in a formatted table in the CLI.
*   **Reasoning**: Improves readability for users compared to raw log lines, making it easier to compare research timings between players at a glance.

## Unit Queuing Extraction (Apr 30, 2026)

### 1. Queuing Action: DeQueue
*   **Decision**: Human player unit queuing is extracted using the `DeQueue` action packet from the replay parser.
*   **Reasoning**: This reliably captures queued units including their count/amount, rather than the exact moment they finish training.

### 2. Building Instance Tracking
*   **Decision**: Implemented an internal `instance_map` that tracks Building Instance ID -> Building Type Name.
*   **Reasoning**:
    *   This map is populated during `DeQueue` events (which contain the building type) and used to resolve building names for `Research` and `Unqueue` events (which only contain instance IDs).
    *   Ensures consistent identification of buildings across different event types.

### 3. Villager Gender Mapping
*   **Decision**: Maintain both ID 83 (Male Villager) and ID 293 (Female Villager) in the `data/units.csv` mapping.
*   **Reasoning**: 
    *   The `DeQueue` action primarily uses the base ID (83) regardless of the spawned gender, as the engine resolves the gender choice internally when the unit spawns.
    *   Mapping both covers any unexpected variations in different replay builds or AI interactions.

### 4. CLI Output
*   **Decision**: Enhanced the unified timeline to include "Building Type(s)" and "Building ID(s)" columns for all queuing and cancellation events.
*   **Reasoning**: Provides a highly detailed log of exactly *when* and *where* every queue, research, and cancellation event occurs, improving debuggability and game analysis. Comma-separated lists are used for multi-building selections.

## Replay/Telemetry Sync (May 1, 2026)

### 1. Two-Point Temporal Calibration
*   **Decision**: Synchronize replay events and screen telemetry using a two-point linear fit (Anchor 1: Start/First Drop, Anchor 2: Feudal Age Research).
*   **Reasoning**: 
    *   Hardcoding the "1.7x" speed factor is unreliable as game clock speed can drift slightly or be adjusted (1.0, 1.5, 2.0).
    *   Using two distinct anchors allows calculating a precise local `speed_factor` and `RW_GameStart` offset, ensuring sub-second alignment throughout long games.
    *   Linear interpolation across the two anchors handles initial "frozen" frames at game start and varied loading times.

### 2. "First Frame" Ground Truth for Resources
*   **Decision**: Extract starting resources (Food, Wood, Gold, Stone) from the first valid frame of screen telemetry rather than the replay header.
*   **Reasoning**:
    *   Binary extraction of starting resources from `.aoe2record` headers is fragile and version-dependent for DE (requires mapping individual `PlayerInit` blocks which vary by player count).
    *   Using the vision pipeline's first reading is 100% accurate for the current civilization and game mode (e.g., Chinese starts, Hindustani bonuses, Empire Wars) because it is what the player actually saw.
    *   Simplifies the synchronization logic and reduces dependency on unstable library-specific binary parsing.

### 3. Causal Ordering (Event-before-Update)
*   **Decision**: In the merged causal stream, sort `RecEvents` before `ScreenGrabs` when they occur in the same millisecond.
*   **Reasoning**:
    *   Ensures that an "Action" (e.g., clicking the 'Build House' button) is indexed before the resulting "State Update" (e.g., resource count dropping).
    *   Simplifies downstream logic for determining if a player was "Out of Food" at the exact moment they failed to queue a villager.
