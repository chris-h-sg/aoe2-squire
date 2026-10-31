# AoE2 Squire Roadmap

This roadmap outlines the development path for the AoE2 Squire, prioritizing core metrics and automation to provide performance data to players, before expanding into complex computer vision features.

## Future Enhancements

Once the MVP is providing user value, development will shift toward expanding our data capture capabilities to unlock more complex coaching features.

### Core Automation & Fundamentals
*   **Pause Detection:** Implement logic to detect game pauses (e.g., via menu detection or timer stagnation) to prevent temporal drift in the synchronization pipeline.
*   **Civilization Tech Tree Data Integration:** Integrate a comprehensive data file defining what units and technologies each civilization has access to, preventing the tool from penalizing players for missing unavailable upgrades.
*   **Starting Resource Extraction:** Update the `aoe2rec` parser to correctly extract starting resources for the recorded player, replacing the current "first frame" heuristic with absolute ground truth.

### Vision Pipeline Expansions
*   **Game Timer (F11) Capture:** Read the top-screen game clock to enable absolute synchronization between vision data and replay data, replacing heuristic syncing.
*   **Global Queue Detection:** Read the top-left global queue to verify when a unit (specifically villagers) or technology is actively in production.
*   **Current Age Detection:** Read the top-center UI element to determine exact Age-up completion times (rather than relying on queue timings).
*   **Bottom Selection Panel Detection:** Ground-truth active building and unit counts when the player uses "Select All" hotkeys.
*   **Digit Recognition Review (after the next recorded games):** The pipeline locates the panel, filters text colours, splits each number into single characters (connected components plus recursive valley splitting for touching digits), then matches each character against one template set scaled down from the large "enormous" font. For one known font this approach is sound: about 1 wrong field in 900 on the supported fixtures, and a live capture matched an earlier one on 99.97% of readings. The likely local maximum is the split-first structure: each splitting heuristic patches one case and risks another, and scaled-down templates don't match how the game draws small numbers.
    *   **Decide with data:** run the vision accuracy baseline and the sanity report on the new recordings (other resolutions and UI sizes, large late-game numbers). If misreads cluster in splitting or in particular sizes, build a replacement side by side and switch only if it beats the current pipeline on all fixtures. If not, keep fixing individual cases.
    *   **Replacement design:** keep panel location and colour filtering; slide every template across the whole number and pick the best-scoring digit sequence, so nothing needs splitting; harvest templates from real frames at native size per UI scale (recording mode makes this easy); report a confidence per digit. A trained classifier is only worth it to support many fonts or mods.

### Advanced Player Issue Analysis
*   **APM Tracking:** Track and graph Actions Per Minute (APM) using the raw interaction events parsed from the replay file.
*   **APM Breakdown:** Categorize player actions into Military (unit commands, combat micro) and Economic (building placement, gathering, unit queuing) to visualize where their attention is focused during different phases of the game.
*   **Basic Idle TC Inference:** Combine the vision pipeline's `population_vils` count with the replay parser's villager queue events. If the villager count stagnates and no queue event is active, flag the TC as idle.
*   **Delayed Economic Upgrades Analysis:** Track technology queue events and correlate them with Age Detection to penalize late essential upgrades (Double-Bit Axe, Horse Collar) accurately.
*   **Poor Build Order Execution:** Compare player Age-up and construction sequences against a database of standard benchmarks.
*   **Unit Counter Analysis:** Parse military queue events and use the Civ Tech Tree data to recommend available unit counters based on the opponent's composition.

### Developer Tooling
*   **Built-in Video Capture (done):** `--record-video <dir>` behind the `dev-tools` Cargo feature (`src/capture/video.rs`) starts ffmpeg (6+, `ddagrab` with a GPU encoder) when a match starts and stops it when it ends or on Ctrl+C. It writes `telemetry_<start>.mkv` with a sidecar holding the wall-clock start time. See "Recording a game" in `test_bench/README.md`.
*   **Video/Telemetry Alignment Tool (done):** `tools/align_video.py` matches panel readings from video frames against the CSV, gives the video offset to within about ±50 ms, checks for drift, and converts CSV / sanity report times to video times. It also works for Steam recordings.
*   **Record Sample Games:** 2–3 games covering game speeds, pauses, repeated Age-up clicks, refunds and more, following the checklist in `test_bench/README.md` ("Sample games to record"). They become fixtures for the sync rework and the sanity report, and the evidence for the Digit Recognition Review.

### Research Tasks
*   **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
*   **AoE2 Object ID Sequence:** Investigate how the engine assigns IDs to foundations and units to enable exact mapping without explicit Interact actions.

---

## Technical Debt & Calibration Quirks

As we bridge the gap between vision-based telemetry and binary replay data, we are maintaining a list of specific technical hurdles to resolve:

*   **Investigate `aoe2rec` Player Stats Parsing**: The forked library currently has trouble reading the correct `PlayerInit` blocks sequentially for DE replays. We've bypassed this by using "First Frame Ground Truth" from telemetry to find starting resources, but we should eventually solve the binary extraction for cleaner logic.
*   **Sync Granularity Tuning**: Capture runs every 250 ms; evaluate whether that is enough for villager production analysis or whether specific micro-gap detection needs a higher rate.
*   **Sync Rework (next)**: The two-point calibration assumes one constant speed. The known failures are the ignored tests in `tests/sync_stress.rs`; `cargo test --test sync_stress -- --ignored` lists them. They cover other game speeds, pauses, unread or garbled values, and a late capture start. The rework must also handle a speed that varies: replay playback runs at its own speed, which is not the game speed and can drift. The sample game's measured 1.690 is such a playback speed. Candidate approaches: reading the in-game clock (F11), and aligning replay spends with resource drops over the whole game (the sanity report's spend alignment).
*   **Cancelled Final Age-up Click**: Only the last click per age counts as the age-up (earlier clicks were cancelled). If that last click was cancelled too and never repeated, the age-up is still counted. Detecting it would mean matching a queue cancellation at the Town Center.
*   **Typed Readings at the Capture Boundary**: The analysis works on typed observations, but capture still writes the telemetry CSV straight from the vision pipeline's string map (`Results`), and the frame recorder parses that map again (`PanelValues`). A typed panel reading would remove the duplication.
*   **Duplicated Age Names**: `constants::GAME_AGES` duplicates the names of the `Age` enum.
*   **Confidence-Based Rejection in Vision**: The matcher always returns its best guess, so a doubtful digit becomes a confidently wrong number. Report low-confidence reads as unread instead. Do it after the sync rework, because sync currently treats an unread value as a resource drop.
*   **Vision CPU Cost**: The live test measured 25–38 ms per frame, about 10% of one core at 4 frames per second. The anchor search, calibration and the main pass each convert the whole frame to RGB again (`to_rgb8`, about 6 MB at 1080p), although only a strip at the top is used. Profile it, then crop the strip once before any processing.
*   **Sanity Report on Every Recorded Game**: `tests/sanity_report.rs` only checks the sample game. Extend it to every game with a `test_bench/snapshots/<game>.sanity.json`, once more games are recorded (see "Sample games to record" in `test_bench/README.md`).

