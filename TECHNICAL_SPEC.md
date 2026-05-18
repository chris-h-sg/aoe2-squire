# Technical Specification

## Data Extraction Pipeline
- **Frame Capture:** Utilize Windows DXGI for low-overhead screen capture via `windows-rs`.
- **UI Calibration:** "Detect First, Extract Second" architecture establishes coordinate origin, UI scale, vertical bounding baselines, and active UI mods (e.g., Anne_HK) before processing digits.
- **Digit Recognition:** Uses native Rust Template Matching via Sum of Squared Differences (SSD) over pixel crops, featuring a ±1 pixel vertical wiggle and symmetry tie-breakers for 100% resolution-independent accuracy.
- **Efficiency Logic:** Compare "Ground Truth" (scraped) vs "Theoretical Max" (parsed via tech logs).

## Replay Parsing
- **Trigger**: Automatic discovery after capture ends, with a manual fallback (native file picker).
- **Filtering**: Ignores replay events beyond the final captured frame to support truncated sessions.
- **Capabilities**: Handles modern DE replay binary formats, extracts a universal `ReplayEvent` stream, and maps IDs using community-standard JSON mapping.


## Unified Telemetry Architecture
- **Goal:** Create a "Universal RTS Telemetry Layer."
- **Strategy:** Build a core engine in Rust with game-specific "Adapters."
- **Implementation:** Use `ui_map.json` to store absolute coordinates for a baseline 1080p resolution, scaling them dynamically at runtime based on the detected anchor.
- **Primary Data Source (AoE2):** **Passive Screen Scraping** is the required default to ensure broad accessibility. (CaptureAge Pro API is treated only as an optional "High-Fidelity" fallback for power users, if available).

## Technical Risks & Considerations
- **CPU Contention:** AoE2 is single-threaded; background scraping must be <1% CPU to avoid micro-stutters.
- **Desyncs:** Real-time state might lag behind the live game in late-game scenarios.
- **Anti-Cheat:** Avoid memory hooking to minimize ban risk; DXGI capture is used passively.

## Fallback Mechanisms
- **Telemetry-Only Fallback**:
  - **Condition**: Triggers when both automated discovery and manual replay file selection are skipped or fail.
  - **Execution**: Employs a fallback speed factor constant of `1.7` (representing standard Normal speed in AoE2:DE) to calibrate real-time timestamps into in-game milliseconds.
  - **Visualization**: Hides per-age stat breakdown tables and presents total/aggregate metrics only. Overrides player labels to a standard active player identifier.