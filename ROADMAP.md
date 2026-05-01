# RTS Analyzer Roadmap

This roadmap outlines the development path for the RTS Analyzer, prioritizing high-value, low-effort features to deliver actionable insights to players as quickly as possible, before investing in deeper automation and complex computer vision expansions.

## Immediate Priorities (Ordered)

The immediate focus is on creating a "Minimum Viable Analysis" that combines our existing data sources to highlight the most critical low-Elo mistakes.

1. **Data Synchronization Foundation:** 
   - Establish a method to synchronize the timestamp data from the DXGI vision pipeline with the exact event timestamps from the parsed `.aoe2record` file. This is a prerequisite for all combined analysis.
2. **Idle Villagers Tracking (Quick Win):** 
   - Utilize our existing `idle_vils` OCR data to calculate and report the total duration and count of idle villagers throughout the game. This requires almost no new parsing logic and provides immediate, high-impact value.
3. **Getting "Housed" Penalties (Quick Win):** 
   - Utilize our existing `detect_housed_overlay` vision function to track the exact duration a player is population-capped. Report the total production time lost.
4. **Floating Resources Alerts (Quick Win):** 
   - Track resource banks using the vision pipeline and flag sustained periods where resources exceed healthy thresholds (e.g., floating 1000+ wood in Feudal Age). We will need to define basic static thresholds.
5. **Basic Idle TC Inference:** 
   - Combine the vision pipeline's `population_vils` count with the replay parser's villager queue events. If the villager count stagnates and no queue event is active, flag the TC as idle.

---

## Future & Uncategorized Enhancements

Once basic analysis is providing user value, development will shift toward automation and expanding our data capture capabilities to unlock more complex coaching features.

### Core Automation & Fundamentals
*   **Automatic Game Start/End Detection:** Implement logic (e.g., process monitoring or specific UI trigger detection) to automatically start and stop the vision capture pipeline.
*   **Automated Replay Fetching:** Automatically locate, identify, and parse the correct `.aoe2record` file that corresponds to the captured gameplay session.
*   **Civilization Tech Tree Data Integration:** Integrate a comprehensive data file defining what units and technologies each civilization has access to, preventing the analyzer from penalizing players for missing unavailable upgrades.

### Vision Pipeline Expansions
*   **Game Timer (F11) Capture:** Read the top-screen game clock to allow flawless, absolute synchronization between vision data and replay data, replacing heuristic syncing.
*   **Global Queue Detection:** Read the top-left global queue to definitively prove when a unit (specifically villagers) or technology is actively in production.
*   **Current Age Detection:** Read the top-center UI element to determine exact Age-up completion times (rather than relying on queue timings).
*   **Bottom Selection Panel Detection:** Ground-truth active building and unit counts when the player uses "Select All" hotkeys.

### Advanced Player Issue Analysis
*   **Delayed Economic Upgrades Analysis:** Track technology queue events and correlate them with Age Detection to penalize late essential upgrades (Double-Bit Axe, Horse Collar) fairly.
*   **Poor Build Order Execution:** Compare player Age-up and construction sequences against a database of standard benchmarks.
*   **Unit Counter Analysis:** Parse military queue events and use the Civ Tech Tree data to recommend available unit counters based on the opponent's composition.

---

## Technical Debt & Calibration Quirks

As we bridge the gap between vision-based telemetry and binary replay data, we are maintaining a list of specific technical hurdles to resolve:

*   **Investigate `aoe2rec` Player Stats Parsing**: The forked library currently has trouble reading the correct `PlayerInit` blocks sequentially for DE replays. We've bypassed this by using "First Frame Ground Truth" from telemetry to find starting resources, but we should eventually solve the binary extraction for cleaner logic.
*   **Dynamic Civilization Bonuses**: Currently using a temporary workaround (46 food) for Hindustani villager costs. This needs to be moved to a dynamic lookup system once civilization tech tree data is integrated.
*   **Sync Granularity Tuning**: Evaluate if 1-second snapshots are sufficient for high-level villager production analysis or if we need higher resolution for specific micro-gap detection.

