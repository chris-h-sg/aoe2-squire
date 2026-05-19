# Decision Log

This log captures the high-level architectural and design decisions of the AoE2 Squire. Detailed implementation notes (e.g., exact pixel thresholds, OCR parsing nuances) are codified and documented within the source code.

## Replay/Telemetry Sync

### 1. Two-Point Temporal Calibration
*   **Decision**: Synchronize replay events and screen telemetry using a two-point linear fit (Anchor 1: Start/First Drop, Anchor 2: Feudal Age Research).
*   **Reasoning**: 
    *   Hardcoding the "1.7x" speed factor is unreliable as game clock speed can drift slightly or be adjusted (1.0, 1.5, 2.0).
    *   Using two distinct anchors allows calculating a precise local `speed_factor` and `RW_GameStart` offset, ensuring sub-second alignment throughout long games.
    *   Linear interpolation across the two anchors handles initial "frozen" frames at game start and varied loading times.

### 2. "First Frame" Ground Truth for Resources
*   **Decision**: Extract starting resources (Food, Wood, Gold, Stone) from the first valid frame of screen telemetry rather than the replay header.
*   **Reasoning**:
    *   Binary extraction of starting resources from `.aoe2record` headers is fragile and version-dependent for DE.
    *   Using the vision pipeline's first reading is 100% accurate for the current civilization and game mode because it is exactly what the player saw.
    *   Simplifies synchronization logic and reduces dependency on unstable binary parsing.

### 3. Causal Ordering (Event-before-Update)
*   **Decision**: Sort `RecEvents` before `ScreenGrabs` in the merged causal stream when they occur in the same millisecond.
*   **Reasoning**:
    *   Ensures that an "Action" (e.g., clicking 'Build House') is indexed before the resulting "State Update" (e.g., resource count dropping).
    *   Simplifies downstream logic for determining if a player was "Out of Food" at the exact moment they failed to queue a villager.

## Idle Villager Metrics

### 1. Metric: Idle Villager Seconds (VS)
*   **Decision**: Calculate "Idle Villager Seconds" as the primary productivity metric.
*   **Reasoning**: Provides a single, objective number to quantify player inefficiency, comparable across different game lengths and civilizations.

### 2. Breakdown: Per-Age Attribution
*   **Decision**: Attribute idle time to specific game ages (Dark, Feudal, Castle, Imperial).
*   **Reasoning**: Helps players identify *when* their management fails (e.g., "I played a perfect Dark Age but lost 500 VS in early Castle Age").

## Floating Resources Analysis

### 1. Threshold Design: Per-Age Static Limits
*   **Decision**: Flag sustained resource accumulation using static per-age thresholds (defined in `constants.rs`).
*   **Reasoning**:
    *   **Dark Age**: Exceeding your starting amount, sustained, is an unambiguous sign of stalled spending.
    *   **Imperial flat-rule**: Late-game complexity makes tighter thresholds produce excessive false positives. A player floating high resources for 30s in Imperial is genuinely stalling.

### 2. Save-Up Window Suppression
*   **Decision**: Trim floating resource runs that overlap with the 60-second window before a major spend event (e.g., Age Up, Castle Placement).
*   **Reasoning**: Surgically removes false positives caused by intentional resource accumulation for major research or construction, while preserving attribution of genuine pre-save-up floating.

### 3. Age Transition Closes Run State
*   **Decision**: When a `RecEvent` advances the age, close and count any in-progress "above threshold" runs that meet the minimum duration in the *previous* age's bucket.
*   **Reasoning**: If a player has already been floating for a long duration when they click an age-up, that float was a legitimate issue in the current age. Closing it at the transition timestamp provides more accurate attribution.

## Pipeline Modularization

*   **Decision**: Utilize a unified library-first architecture with thin binary wrappers and a "Progressive Disclosure" CLI (verbose flag), instead of standalone analysis binaries.
*   **Reasoning**: Enables in-memory orchestration of the full analysis suite immediately after capture, eliminates redundant I/O, and centralizes shared analytical logic (segmentation, timing) for better maintainability and testability.

## HTML Report Visualization

### 1. Unified Multi-Layer Timeline
*   **Decision**: Overlay population metrics, research markers, and status lanes (housed/floating) into a single synchronized timeline.
*   **Reasoning**: Enables immediate visual correlation between different performance gaps (e.g., clicking an age-up and the resulting resource float or villager idle spike).

### 2. Hierarchical Tooltip Design
*   **Decision**: Categorize tooltip information into distinct sections (Body, AfterBody, and Footer).
*   **Reasoning**: Prevents information density overload when multiple alerts (housed + floating) are active simultaneously, ensuring the most critical statuses remain prominent.

### 3. Offline Asset Bundling
*   **Decision**: Embed the Chart.js library directly into the HTML report.
*   **Reasoning**: Ensures the report is fully functional without internet access, aligning with the "Single Executable" goal.

### 4. Type-Safe Squire Grading Engine
*   **Decision**: Grading engine in Rust to compute match performance metrics (Idle Percentage, Housed Percentage).
*   **Reasoning**: Avoids complex logic inside the HTML report template and improves testability.

## Distribution & Packaging

### 1. Report Theming
*   **Decision**: Style the HTML report with a "medieval parchment" aesthetic (parchment background, dark wood panels, Cinzel serif headers).
*   **Reasoning**: The tool targets players who want to improve performance; the design should feel approachable and evoke the setting without mimicking the game UI directly.

### 2. Asset Embedding
*   **Decision**: Use compile-time embedding (`include_dir`, `include_str!`) for UI maps and OCR templates instead of filesystem loading.
*   **Reasoning**: Ensures a "Zero Setup" user experience. Prevents accidental deletion of vital assets and maintains a minimal binary size.

### 3. Manual Replay Selection Fallback
*   **Decision**: Provide a Windows file chooser fallback when automated replay discovery fails, defaulting to the most recently modified savegame directory.
*   **Reasoning**: Handles edge cases for advanced users (casters, analysts) where the replay file might be manipulated before the capture starts, without breaking the automated flow.

## UI Mod Support

### 1. Dynamic Mod Detection (Anne_HK)
*   **Decision**: Detect `Anne_HK resource panels` via pixel-color signatures in the wood villager panel and automatically adjust the vision pipeline.
*   **Reasoning**: 
    *   Many competitive players use this mod that changes text colors and clears out panel backgrounds.
    *   By detecting the mod once per frame, we can switch to an "Ignore Color" (Greyscale) extraction mode and adjust bounding box heights to accommodate larger modded fonts, without requiring the user to switch profiles or configuration files manually.

## Telemetry-Only Fallback Report Mode

### 1. Telemetry-Only Execution Fallback
*   **Decision**: Generate an HTML match report using a standard "1.7" game speed factor when no recorded game (`.aoe2record`) file is found or selected.
*   **Reasoning**:
    *   Prevents empty exits when automated replay discovery and manual selection both fail or are skipped.
    *   We can still provide user analytics (total idle villager time, housing efficiency, resource floating, population charts) derived from real-time screen telemetry.

## Match Winner Detection

### 1. Replay Resign Event Extraction
*   **Decision**: Determine the match winner by observing `Resign` events in the `aoe2record` parsed command stream.
*   **Reasoning**: 
    *   The `aoe2record` header does not expose the match victor. We infer the match outcome for 1v1s by identifying the `Resign` action from any player to enable marking the winning player in the post-match HTML report.
