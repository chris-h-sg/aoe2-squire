# RTS Analyzer Roadmap

This roadmap outlines the development path for the RTS Analyzer, prioritizing high-value, low-effort features to deliver actionable insights to players as quickly as possible, before investing in deeper automation and complex computer vision expansions.

## Immediate Priorities (Release Prototype)

The immediate focus is to package our existing analyzers into a seamless, automated flow for our first private prototype release.

1. ✅ **Remove Civilization Workarounds:** Removed the temporary Hindustani villager cost workaround, expecting standard civs only for a stable baseline.
2. ✅ **Master Orchestrator (Part 1): Game State Automation:** Implemented UI anchor and field detection to automatically start/stop the capture loop, including a CLI spinner for user feedback.
3. ✅ **Master Orchestrator (Part 2): Replay Discovery:** Implemented logic to locate the user's Steam SaveGame directory and identify the most recent `.aoe2record` file immediately after a match concludes.
4. ✅ **Master Orchestrator (Part 3): Integrated Analysis Pipeline:** 
    - Created a unified execution flow that programmatically runs the `mesher` and all three analyzers (Idle, Housing, Floating) in sequence without manual CLI intervention.
5. **HTML Report Generation:** Generate a clean, unified HTML report summarizing all findings and auto-open it in the default browser at the end of the pipeline.
6. **Embed Assets & Data:** Move all external dependencies into the binary using `include_str!` or `include_bytes!` to ensure a single standalone `.exe`:
    - CSV Data Files (`data/*.csv`)
    - UI Mapping JSON (`ui_map.json`)
    - OCR PNG Templates (`research/templates/enormous_numbers/*.png`)
7. **Executable Packaging:** Finalize the release build into a single standalone `.exe`.

---

## Completed

*   ✅ **Data Synchronization Foundation:** *(complete — `mesher` binary)*
    - A two-point linear calibration model aligns vision telemetry with `.aoe2record` events at sub-second precision. Output: `output/merged_observations.csv`.
*   ✅ **Idle Villagers Tracking (Quick Win):** *(complete — `idle_analyzer` binary)*
    - Reports villager-seconds (VS) lost per age with a full chronological segment log and age-research click annotations.
*   ✅ **Getting "Housed" Penalties (Quick Win):** *(complete — `housing_analyzer` binary)*
    - Reports seconds spent in `housed` and `queued` population states, broken down by game age.
*   ✅ **Floating Resources Alerts (Quick Win):** *(complete — `floating_analyzer` binary)*
    - Tracks resource banks using the vision pipeline and flags sustained periods (30s+) where resources exceed age-specific healthy thresholds.

---

## Future & Uncategorized Enhancements

Once the MVP is providing user value, development will shift toward expanding our data capture capabilities to unlock more complex coaching features.

### Core Automation & Fundamentals
*   **Pause Detection:** Implement logic to detect game pauses (e.g., via menu detection or timer stagnation) to prevent temporal drift in the synchronization pipeline.
*   **Civilization Tech Tree Data Integration:** Integrate a comprehensive data file defining what units and technologies each civilization has access to, preventing the analyzer from penalizing players for missing unavailable upgrades.
*   **Starting Resource Extraction:** Update the `aoe2rec` parser to correctly extract starting resources for the recorded player, replacing the current "first frame" heuristic with absolute ground truth.

### Vision Pipeline Expansions
*   **Game Timer (F11) Capture:** Read the top-screen game clock to allow flawless, absolute synchronization between vision data and replay data, replacing heuristic syncing.
*   **Global Queue Detection:** Read the top-left global queue to definitively prove when a unit (specifically villagers) or technology is actively in production.
*   **Current Age Detection:** Read the top-center UI element to determine exact Age-up completion times (rather than relying on queue timings).
*   **Bottom Selection Panel Detection:** Ground-truth active building and unit counts when the player uses "Select All" hotkeys.

### Advanced Player Issue Analysis
*   **Basic Idle TC Inference:** Combine the vision pipeline's `population_vils` count with the replay parser's villager queue events. If the villager count stagnates and no queue event is active, flag the TC as idle.
*   **Delayed Economic Upgrades Analysis:** Track technology queue events and correlate them with Age Detection to penalize late essential upgrades (Double-Bit Axe, Horse Collar) fairly.
*   **Poor Build Order Execution:** Compare player Age-up and construction sequences against a database of standard benchmarks.
*   **Unit Counter Analysis:** Parse military queue events and use the Civ Tech Tree data to recommend available unit counters based on the opponent's composition.

---

## Technical Debt & Calibration Quirks

As we bridge the gap between vision-based telemetry and binary replay data, we are maintaining a list of specific technical hurdles to resolve:

*   **Investigate `aoe2rec` Player Stats Parsing**: The forked library currently has trouble reading the correct `PlayerInit` blocks sequentially for DE replays. We've bypassed this by using "First Frame Ground Truth" from telemetry to find starting resources, but we should eventually solve the binary extraction for cleaner logic.
*   **Sync Granularity Tuning**: Evaluate if 1-second snapshots are sufficient for high-level villager production analysis or if we need higher resolution for specific micro-gap detection.

