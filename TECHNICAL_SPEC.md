# Technical Specification

## Data Extraction Pipeline
1. **Frame Capture:** Utilize Windows DXGI (Desktop Duplication API) for low-overhead screen capture via `windows-rs`.
2. **UI Anchoring:** Detect a static UI element (the "Age" icon cluster) to dynamically establish a coordinate origin and UI scale.
3. **Normalized Cropping:** Extract resource bar regions using scale-adjusted offsets from the anchor to support variable resolutions (1080p, 1440p, 4K) and UI scalings.
4. **Digit Recognition:** Use native Rust Template Matching via Sum of Squared Differences (SSD) over 1:1 pixel crops, avoiding OpenCV overhead. Uses `image` and `imageproc` crates.
5. **Efficiency Logic:** Compare "Ground Truth" (scraped) vs "Theoretical Max" (parsed via tech logs).
    - *Formula:* Efficiency = (Vils_Scraped * BaseRate_ParsedTech) / DeltaResource_Scraped.

## 6. Replay Parsing
- **Trigger**: File watcher (future) or manual CLI command.
- **Library**: Custom integration with the **liouh/aoe2rec** Rust fork.
- **Capabilities**:
    - **Re-sync Logic**: Handles modern DE replay binary formats with checksum validation.
    - **Event Extraction**: Maps raw binary actions to a universal `ReplayEvent` stream (Techs, Units, Build commands).
    - **ID Translation**: Uses a community-standard JSON mapping (from `aoe2techtree`) for accurate engine-to-English translation.


## Unified Telemetry Architecture
- **Goal:** Create a "Universal RTS Telemetry Layer."
- **Strategy:** Build a core engine in Rust with game-specific "Adapters."
- **Implementation:** Use `ui_map.json` to store absolute coordinates for a baseline 1080p resolution, scaling them dynamically at runtime based on the detected anchor.
- **Primary Data Source (AoE2):** **Passive Screen Scraping** is the required default to ensure broad accessibility. (CaptureAge Pro API is treated only as an optional "High-Fidelity" fallback for power users, if available).

## Technical Risks & "Known Unknowns"
- **CPU Contention:** AoE2 is single-threaded; background scraping must be <1% CPU to avoid micro-stutters. (Mitigated by Rust + DXGI).
- **Desyncs:** Real-time state might lag behind the live game in late-game scenarios.
- **UI Scaling:** Handled dynamically by scanning for the anchor in the top 5% of the frame.
- **Anti-Cheat:** Confirm if DXGI capture is explicitly whitelisted. Avoid memory hooking to minimize ban risk.
- **Code Signing:** An EV (Extended Validation) Certificate (~$500/yr) is required to avoid Windows SmartScreen warnings.