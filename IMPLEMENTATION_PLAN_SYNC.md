# Implementation Plan: Telemetry & Replay Sync (Phase 4)

**Objective:** Validate the vision pipeline's accuracy and latency by comparing real-time scraped telemetry (CSV) with the absolute "Ground Truth" extracted from game recordings (`.aoe2record`).

---

## 1. Telemetry Data Export (Live Capture) [COMPLETED]
Update the Rust capture loop to persist data for offline analysis.
- **Action:** Modify `src/capture/mod.rs` to initialize a `csv::Writer` when `--live` is active.
- **Output:** `logs/telemetry_YYYYMMDD_HHMMSS.csv` (timestamped filenames)
- **Columns:**
    - `timestamp_ms`: System wall-clock time.
    - `food_total`, `food_vils`
    - `wood_total`, `wood_vils`
    - `gold_total`, `gold_vils`
    - `stone_total`, `stone_vils`
    - `pop_curr`, `pop_max`, `pop_vils`
    - `idle_vils`
- **Rate:** Log every frame processed (~2 FPS).
- **Validation:** Added unit tests for CSV row preparation and population splitting.

## 2. Replay Event Extraction
Extract game-time events from the recording file.
- **Action:** Create a utility (or new binary) using the `aoe2rec` crate.
- **Input:** `<match_name>.aoe2record`
- **Extraction Targets:**
    - **Building Actions:** Type, Start Time, Cost (to verify resource drops).
    - **Unit Training:** Type, Start Time (to verify pop changes).
    - **Tech Research:** Type, Completion Time (to verify gather rate increases).
    - **Starting State:** Initial resources and player civ.

## 3. The "Mesh" & Validation Engine
Build a tool to align the two timelines and quantify error rates.
- **Time Synchronization (T=0 Anchor):**
    - The tool will scan for the first "Resource Drop" in the CSV (e.g., -25 Wood for a house) and match it to the first "Build Action" in the Replay to establish the time offset.
- **Validation Logic:**
    - **Consistency Check:** Did a building action at `T=120s` correlate with a resource drop in the CSV at `T_offset + 120s`?
    - **Latency Measurement:** What is the average delay between a Replay event and the Vision pipeline detecting it?
    - **OCR Error Rate:** Identify "Flickers" (where a value jumps incorrectly for one frame) by comparing against the monotonic resource changes expected from the Replay.

---

## Next Steps for New Session:
1.  **Gather Test Data**: Record a short 5-minute AoE2 game while running the analyzer to generate a paired `.csv` (in `logs/`) and `.aoe2record` set.
2.  **Implement Sync Tool**: Build a "Mesher" that aligns timestamps from the CSV with events in the `.aoe2record` to verify resource drops.
