# Technical Specification

## Data Extraction Pipeline
1. **Frame Capture:** Utilize Windows DXGI (Desktop Duplication API) for low-overhead screen capture.
2. **UI Anchoring:** Detect a static UI element (e.g., the "Age" icon) to establish a coordinate origin.
3. **Normalized Cropping:** Define resource bar regions using percentage-based offsets from the anchor to support variable resolutions (1080p, 1440p, 4K).
4. **Digit Recognition:** Use Template Matching (OpenCV) for unit counts and resources.
5. **Efficiency Logic:** Compare "Ground Truth" (scraped) vs "Theoretical Max" (parsed via tech logs).
    - *Formula:* Efficiency = (Vils_Scraped * BaseRate_ParsedTech) / DeltaResource_Scraped.

6. **Replay Parsing**
- Trigger: File watcher monitors the game's `/SaveGame` directory for new `.aoe2record` files.
- Library: Use `mgz` (Python) or a Rust port (`aoe2-parser`) to extract tech research timestamps and unit production commands.

## Unified Telemetry Architecture
- **Goal:** Create a "Universal RTS Telemetry Layer."
- **Strategy:** Build a core engine with game-specific "Adapters."
- **Implementation:** Use a `ui_map.json` to store coordinate anchors (e.g., the "Age" icon) and percentage-based offsets to support resolution independence (1080p, 4K, UI scaling).
- **Primary Data Source (AoE2):** **Passive Screen Scraping** is the required default to ensure broad accessibility. (CaptureAge Pro API is treated only as an optional "High-Fidelity" fallback for power users, if available).

## Technical Risks & "Known Unknowns"
- **CPU Contention:** AoE2 is single-threaded; background scraping must be <1% CPU to avoid micro-stutters.
- **Desyncs:** Real-time state might lag behind the live game in late-game scenarios.
- **UI Scaling:** How to dynamically detect if a user has "UI Scale" set to 125% or 150% in-game?
- **Anti-Cheat:** Confirm if GDI+/DXGI capture is explicitly whitelisted. Avoid memory hooking to minimize ban risk.
- **Code Signing:** An EV (Extended Validation) Certificate (~$500/yr) is required to avoid Windows SmartScreen warnings.