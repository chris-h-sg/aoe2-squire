# RTS Analyzer Roadmap

This roadmap outlines the development path for the RTS Analyzer, prioritizing core metrics and automation to provide performance data to players, before expanding into complex computer vision features.

## Future Enhancements

Once the MVP is providing user value, development will shift toward expanding our data capture capabilities to unlock more complex coaching features.

### Core Automation & Fundamentals
*   **Pause Detection:** Implement logic to detect game pauses (e.g., via menu detection or timer stagnation) to prevent temporal drift in the synchronization pipeline.
*   **Civilization Tech Tree Data Integration:** Integrate a comprehensive data file defining what units and technologies each civilization has access to, preventing the analyzer from penalizing players for missing unavailable upgrades.
*   **Starting Resource Extraction:** Update the `aoe2rec` parser to correctly extract starting resources for the recorded player, replacing the current "first frame" heuristic with absolute ground truth.

### Vision Pipeline Expansions
*   **Game Timer (F11) Capture:** Read the top-screen game clock to enable absolute synchronization between vision data and replay data, replacing heuristic syncing.
*   **Global Queue Detection:** Read the top-left global queue to verify when a unit (specifically villagers) or technology is actively in production.
*   **Current Age Detection:** Read the top-center UI element to determine exact Age-up completion times (rather than relying on queue timings).
*   **Bottom Selection Panel Detection:** Ground-truth active building and unit counts when the player uses "Select All" hotkeys.

### Advanced Player Issue Analysis
*   **APM Tracking:** Track and graph Actions Per Minute (APM) using the raw interaction events parsed from the replay file.
*   **APM Breakdown:** Categorize player actions into Military (unit commands, combat micro) and Economic (building placement, gathering, unit queuing) to visualize where their attention is focused during different phases of the game.
*   **Basic Idle TC Inference:** Combine the vision pipeline's `population_vils` count with the replay parser's villager queue events. If the villager count stagnates and no queue event is active, flag the TC as idle.
*   **Delayed Economic Upgrades Analysis:** Track technology queue events and correlate them with Age Detection to penalize late essential upgrades (Double-Bit Axe, Horse Collar) accurately.
*   **Poor Build Order Execution:** Compare player Age-up and construction sequences against a database of standard benchmarks.
*   **Unit Counter Analysis:** Parse military queue events and use the Civ Tech Tree data to recommend available unit counters based on the opponent's composition.

### Research Tasks
*   **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
*   **AoE2 Object ID Sequence:** Investigate how the engine assigns IDs to foundations and units to enable exact mapping without explicit Interact actions.

---

## Technical Debt & Calibration Quirks

As we bridge the gap between vision-based telemetry and binary replay data, we are maintaining a list of specific technical hurdles to resolve:

*   **Investigate `aoe2rec` Player Stats Parsing**: The forked library currently has trouble reading the correct `PlayerInit` blocks sequentially for DE replays. We've bypassed this by using "First Frame Ground Truth" from telemetry to find starting resources, but we should eventually solve the binary extraction for cleaner logic.
*   **Sync Granularity Tuning**: Evaluate if 1-second snapshots are sufficient for high-level villager production analysis or if we need higher resolution for specific micro-gap detection.

