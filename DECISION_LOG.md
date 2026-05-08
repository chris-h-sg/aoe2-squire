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
    *   We found a multi-stage tolerance to be optimal: **20** for high-quality extraction and **30** for robust segmentation.
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

## Idle Villager Metrics (May 1, 2026)

### 1. Metric: Idle Villager Seconds (VS)
*   **Decision**: Calculate "Idle Villager Seconds" as the primary productivity metric.
*   **Logic**: 
    *   Sum the product of `IdleCount * Duration` for every period of constant idle state.
    *   Transitions are triggered by changes in the vision-detected idle count or game age clicks.
*   **Reasoning**: Provides a single, objective number to quantify player inefficiency, comparable across different game lengths and civilizations.

### 2. Breakdown: Per-Age Attribution
*   **Decision**: Attribute idle time to specific game ages (Dark, Feudal, Castle, Imperial).
*   **Method**: Use `RecEvent` research clicks as absolute temporal boundaries for the VS buckets.
*   **Reasoning**: Helps players identify *when* their management fails (e.g., "I played a perfect Dark Age but lost 500 VS in early Castle Age").

### 3. Code Quality: Structured Data Over Tuples
*   **Decision**: Refactored `mesher` and `replay` modules to use structured structs (`MergedRow`, `ReplayData`) instead of deeply nested tuples.
*   **Reasoning**: 
    *   Resolved Clippy `type_complexity` warnings.
    *   Improved maintainability and readability — named fields (e.g., `row.in_game_ms`) are much clearer than index-based access (e.g., `row.0`).
    *   Ensures type safety when extending the merged stream with new columns (like `idle_vils` and `pop_curr`).

### 4. Logic Extraction for Testability
*   **Decision**: Extracted core temporal logic (Calibration, VS Segmentation) from binary entry points (`main`) into the library (`src/sync.rs`, `src/analysis.rs`).
*   **Reasoning**: 
    *   Binaries are difficult to test; moving logic to the library allowed for a 100% automated test suite for the complex calibration state machine.
    *   Enables reusing the analysis logic in future UI components (e.g., a real-time efficiency dashboard) without code duplication.

## Vision Pipeline (May 2, 2026)

### 1. Separation of Concerns: Vision vs. Game State
*   **Decision**: The image analyzer (`poc_vision.py`) strictly reports observed UI states (e.g., `pop_color: "white" | "yellow" | "overlay"`), deferring high-level state interpretation (e.g., "housed" vs. "queued") to the video/telemetry analyzer.
*   **Reasoning**: 
    *   The game UI flashes between an overlay and yellow text when fully housed, and stays solid yellow when units are "queued" beyond pop capacity.
    *   Frame-by-frame analysis cannot distinguish the "off-frame" of a fully housed player from a merely "queued" player without temporal context.
    *   Outputting raw visual states prevents flickering and data loss, allowing the downstream analyzer to apply a smoothing window or cross-reference with OCR numbers to determine the true state.

### 2. "Queued" State Detection
*   **Decision**: Maintained the simpler `np.any(is_yellow)` check in `contains_yellow` without a strict pixel count threshold.
*   **Reasoning**: We initially thought yellow pixels on white population numbers were video compression artifacts, but discovered they are actual in-game warnings when queued unit capacity exceeds population limits. Tightening the blue margin to `50` perfectly balances ignoring actual compression artifacts while retaining sensitivity for these valid UI warnings.

## Vision & State Analysis (May 4, 2026)

### 1. Sampling Frequency & Timing
*   **Decision**: Increase capture frequency to 4 FPS (250ms interval) and implement a self-correcting loop.
*   **Reasoning**:
    *   **Aliasing**: A 250ms interval ensures we catch the ~400ms "on" window of the housed overlay flash, avoiding Nyquist aliasing.
    *   **Drift**: Using a deadline-based sleep compensates for the ~50ms of vision pipeline latency, preventing sample drift and ensuring tight 250ms gaps in the telemetry CSV.

### 2. Discrete Population Fields
*   **Decision**: Split the `population_total` OCR result into discrete `total` (current) and `housing` (capacity) fields.
*   **Reasoning**: Eliminates brittle string parsing in downstream analysis and ensures `total` always refers to a numeric count, matching other resource fields.

## Standardization & Config (May 4, 2026)

### 1. Millisecond-First Architecture
*   **Decision**: Standardized all internal logic, configurations, and data structures to use `u64` milliseconds.
*   **Reasoning**: 
    *   Eliminates floating-point precision issues and redundant conversions between seconds and ms.
    *   Ensures consistency with the high-resolution timestamps used in the DXGI capture loop and telemetry CSVs.

### 2. Externalized Sensitivity Parameters
*   **Decision**: Moved all vision sensitivity parameters (yellow thresholds, interpolation timeouts) to `src/constants.rs`.
*   **Reasoning**: Allows rapid tuning of detection accuracy across different monitor brightness/contrast settings without recompiling core logic.

### 3. Priority-Based Queued Interpolation
*   **Decision**: Implement a dual-anchor state machine to interpolate both "housed" (overlay) and "queued" (yellow) states, with strict priority for housed.
*   **Logic**:
    *   **Anchors**: `overlay` frames anchor both housed and queued bridges. `yellow` frames anchor only queued bridges.
    *   **Priority**: When a bridge is resolved, `housed` status is applied first. `queued` is applied only if the frame wasn't already upgraded to housed.
    *   **Transition Bridging**: Gaps between an `overlay` and a `yellow` frame (or vice versa) are bridged to `queued`.
    *   **Population Bounds**: Both bridge types enforce $diff(C) \leq \max(diff(A), diff(B))$, where $diff = housing - total$.
*   **Reasoning**:
    *   **Dual-State Flickering**: The game UI flashes between "overlay" and "yellow" when housed. Treating `overlay` as a valid "queued" anchor ensures the queued bridge doesn't break during a housed flicker.

### 4. Decoupled Interpolation Timeouts
*   **Decision**: Decouple the interpolation timeouts for "housed" and "queued" states, setting housed to 1000ms and queued to 2000ms.
*   **Reasoning**:
    *   The "queued" state (yellow text) flickers at a significantly slower and less consistent rate than the "housed" overlay flash.
    *   A 1000ms timeout was insufficient to bridge the gaps in queued status, while a 2000ms timeout reliably connects the state without introducing false positives when units die or houses complete.

## Vision Pipeline (May 4, 2026)

### 3. Robust UI Anchor Detection
*   **Decision**: Replaced BFS connectivity with a **10x10 density check** and added strict **scale bounds (0.5–2.0)**.
*   **Reasoning**: Improves detection on anti-aliased frames where single-pixel gaps would otherwise cause an anchor miss.

## Floating Resources Analysis (May 4, 2026)

### 1. Threshold Design: Per-Age Static Limits
*   **Decision**: Flag sustained resource accumulation using static per-age thresholds stored in `constants.rs` as a `const [ResourceThresholds; 4]` indexed to match `GAME_AGES`.
*   **Thresholds**:

    | Resource | Dark Age | Feudal Age | Castle Age | Imperial Age |
    |---|---|---|---|---|
    | Food | 200 | 500 | 800 | 1000 |
    | Wood | 200 | 500 | 800 | 1000 |
    | Gold | 100 | 300 | 500 | 1000 |
    | Stone | 200 | 300 | 500 | 1000 |

*   **Reasoning**:
    *   **Dark Age values equal starting resources** (200/200/100/200). Exceeding your starting amount, sustained, is an unambiguous sign of stalled spending.
    *   **Food and wood use identical thresholds per age** for player-facing clarity — two primary resources, one rule.
    *   **Gold and stone scale more slowly** because their sinks are narrower (gold: military units; stone: castles/towers). They converge to 1000 in Imperial where late-game macro noise is highest.
    *   **Imperial flat-1000 rule**: Late-game complexity makes tighter thresholds produce excessive false positives. A player floating 1000+ of any resource for 30s in Imperial is genuinely stalling.

### 2. Minimum Floating Duration: 30 Seconds
*   **Decision**: Only flag a resource run if it exceeds the threshold continuously for at least **30,000ms** (`FLOATING_MIN_DURATION_MS`).
*   **Reasoning**: Brief spikes are normal during resource reassignment, unit queuing, or building placement. 30s filters these while reliably catching sustained neglect.

### 3. Save-Up Window Suppression
*   **Decision**: Trim floating resource runs that overlap with the **60-second window before a major spend event** (`SAVE_UP_WINDOW_MS = 60_000`). Applies to:
    *   **Feudal Age click** → food only
    *   **Castle / Imperial Age click** → food + gold
    *   **Castle placement (`Build Castle`)** → stone only
*   **Implementation**:
    *   A pre-indexing pass (`index_save_up_events`) converts major spend events into flat lists of forbidden time intervals (`Vec<(u64, u64)>`) grouped by resource into a `Suppressions` struct.
    *   `apply_suppression(start_ms, end_ms, intervals)` subtracts any overlapping save-up windows from a floating segment, returning a `Vec<(u64, u64)>` of valid sub-segments.
        *   **Full overlap**: segment is discarded (returns empty vec).
        *   **Partial overlap**: segment is trimmed to the window boundary.
        *   **Spanning overlap**: segment is split into pre-window and post-event sub-segments.
        *   **No overlap**: segment is unchanged.
*   **Rejected alternatives**:
    *   **Threshold raise** (+500f before Feudal etc.): changes the meaning of "floating" and couples cost data into threshold logic.
    *   **Full-window discard**: a player who was genuinely floating for 2 minutes before then saving up would go un-flagged for the early portion.
*   **Reasoning**: Surgically removes false positives caused by resource accumulation for age advancement or castle construction, while preserving attribution of genuine pre-save-up floating.

### 4. Age Transition Closes Run State
*   **Decision**: When a `RecEvent` advances the age, any in-progress "above threshold" runs that meet the 30s minimum duration are closed and counted in the *previous* age's bucket. Short runs (< 30s) are discarded.
*   **Reasoning**: If a player has already been floating for more than 30s when they click an age-up research, that float was a legitimate issue in the current age. Closing it at the transition timestamp (rather than discarding it) provides more accurate attribution of economic stalling.

## Pipeline Modularization (May 5, 2026)

*   **Decision**: Transitioned from standalone analysis binaries to a unified library-first architecture with thin binary wrappers and a "Progressive Disclosure" CLI (verbose flag).
*   **Reasoning**: Enables in-memory orchestration of the full analysis suite immediately after capture, eliminates redundant I/O, and centralizes shared analytical logic (segmentation, timing) for better maintainability and testability.

## HTML Report Visualization (May 6, 2026)

### 1. Unified Multi-Layer Timeline
*   **Decision**: Overlaid population metrics, research markers, and status lanes (housed/floating) into a single synchronized timeline.
*   **Reasoning**: Enables immediate visual correlation between different performance gaps (e.g., clicking an age-up and the resulting resource float or villager idle spike).

### 2. Categorized HTML Legend
*   **Decision**: Replaced the default Chart.js legend with a custom HTML-based two-tier legend.
*   **Reasoning**: A single-line legend with 7+ items became unreadable. Category-based grouping (Economy vs. Population) reduces cognitive load and allows for better vertical spacing control.

### 3. Hierarchical Tooltip Design
*   **Decision**: Categorized tooltip information into distinct sections (Body, AfterBody, and Footer).
*   **Reasoning**: Prevents information density overload when multiple alerts (housed + floating) are active simultaneously, ensuring the most critical statuses remain prominent.
### 4. Offline Asset Bundling
*   **Decision**: Embedded the Chart.js library directly into the HTML report using `include_str!`.
*   **Reasoning**: Ensures the report is fully functional without internet access. This aligns with the "Single Executable" requirement by eliminating external CDN dependencies at runtime.

## Vision Pipeline Tuning (May 7, 2026)

### 1. Explicit Anchor Spatial Bounds
*   **Decision**: Replaced the simple `ANCHOR_SCAN_FRACTION` with explicit spatial bounds (`ANCHOR_MIN_X`, `ANCHOR_MAX_X`, `ANCHOR_MIN_Y`, `ANCHOR_MAX_Y`).
*   **Reasoning**: 
    *   Relaxing the red color thresholds to improve stability on video compression artifacts inadvertently caused the engine to pick up red UI elements that appear for some civs (e.g. Koreans).
    *   By restricting the anchor scan area strictly to the top-center/right area (excluding the far right and top), we ignore UI noise.
    *   These boundaries work consistently across standard 16:9 and ultra-wide (21:9, 32:9) aspect ratios because the top-bar resource UI remains anchored.

## Report Theming (May 8, 2026)

### 1. Medieval-Inspired Visual Theme
*   **Decision**: Styled the HTML report with a "medieval parchment" aesthetic — parchment background, dark wood panels, Cinzel serif headers, metallic gold accents, and Roboto for body/data text.
*   **Reasoning**: The tool targets AoE2 players who want to improve their competitive performance. The design should feel approachable and evoke the medieval setting without closely mimicking the game's own UI. A parchment-and-wood palette achieves this while keeping data tables and charts clean and readable.
