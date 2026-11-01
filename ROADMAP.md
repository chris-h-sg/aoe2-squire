# AoE2 Squire Roadmap

This roadmap outlines the development path for the AoE2 Squire, prioritizing core metrics and automation to provide performance data to players, before expanding into complex computer vision features.

## Future Enhancements

Once the MVP is providing user value, development will shift toward expanding our data capture capabilities to unlock more complex coaching features.

### Core Automation & Fundamentals
*   **Pause Detection:** Sync finds pauses between matched spends (see "Sync Rework" below). Where the in-game clock is visible, it could also find them at the very start and end of a game.
*   **Civilization Tech Tree Data Integration:** Integrate a comprehensive data file defining what units and technologies each civilization has access to, preventing the tool from penalizing players for missing unavailable upgrades.
*   **Starting Resource Extraction:** Update the `aoe2rec` parser to correctly extract starting resources for the recorded player, replacing the current "first frame" heuristic with absolute ground truth.

### Vision Pipeline Expansions
*   **Game Timer (F11) Capture (optional):** When the top-screen game clock is visible, read it as extra time pairs for sync. It is off for many players, so it can only add to spend alignment, never replace it.
*   **Global Queue Detection:** Read the top-left global queue to verify when a unit (specifically villagers) or technology is actively in production.
*   **Current Age Detection:** Read the top-center UI element to determine exact Age-up completion times (rather than relying on queue timings).
*   **Bottom Selection Panel Detection:** Ground-truth active building and unit counts when the player uses "Select All" hotkeys.
*   **Digit Recognition Review (after the next recorded games):** The pipeline locates the panel, filters text colours, splits each number into single characters (connected components plus recursive valley splitting for touching digits), then matches each character against one template set scaled down from the large "enormous" font. For one known font this approach is sound: about 1 wrong field in 900 on the supported fixtures, and a live capture matched an earlier one on 99.97% of readings. The likely local maximum is the split-first structure: each splitting heuristic patches one case and risks another, and scaled-down templates don't match how the game draws small numbers. Game 1 adds a reproducible case: the villager count 10, drawn over the edge of the population icon, is read as 111 (`test_bench/game_frames/game1-castle-vs-ai/`, known failures).
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
*   **Record Sample Games:** 2–3 games covering game speeds, pauses, repeated Age-up clicks, refunds and more, following the checklist in `test_bench/README.md` ("Sample games to record"). They become fixtures for the sync rework and the sanity report, and the evidence for the Digit Recognition Review. Games 1 and 2 are recorded (`test_bench/game1-castle-vs-ai.*`, `test_bench/game2-fast-pauses.*`, videos and frames in the data release; see `test_bench/README.md`); game 3 is open.

### Research Tasks
*   **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
*   **AoE2 Object ID Sequence:** Investigate how the engine assigns IDs to foundations and units to enable exact mapping without explicit Interact actions.

---

## Technical Debt & Calibration Quirks

As we bridge the gap between vision-based telemetry and binary replay data, we are maintaining a list of specific technical hurdles to resolve:

*   **Investigate `aoe2rec` Player Stats Parsing**: The forked library currently has trouble reading the correct `PlayerInit` blocks sequentially for DE replays. We've bypassed this by using "First Frame Ground Truth" from telemetry to find starting resources. Sync no longer needs starting resources, so this is low priority.
*   **Sync Granularity Tuning**: Capture runs every 250 ms; evaluate whether that is enough for villager production analysis or whether specific micro-gap detection needs a higher rate.
*   **Sync Rework**:
    *   **Phase 1 (done)**: Sync is a piecewise-linear time mapping fitted to replay spends matched with resource drops over the whole game (DECISION_LOG.md, "Time Mapping from Spend Alignment"). It handles any speed from 0.5 to 4.0, pauses, a speed that changes or drifts (replay playback), unread values and a late capture start, and fails with an error instead of guessing. `tests/sync_stress.rs` has no ignored scenarios left; all of them are simulated by distorting the sample game.
    *   **Phase 2 (next, after the sample games are recorded)**: Check spend alignment against the recorded games at other speeds, with real pauses and with replay playback whose speed changes. Those games are the first real test of pause handling. Game 2 (Fast speed, three real pauses, late capture start) syncs with all three pauses found at the right places and 114/116 spends matched; replay playback with a changing speed (game 3) is still open. Spend alignment stays the main method, because the in-game clock (F11) is off for many players.
    *   **Optional, when the clock is visible**: feed in-game clock readings to the same mapping fit as extra time pairs (`MappingSource`). They would cover stretches without spends and pauses at the very start or end of a game. Games without the clock keep working exactly as now.
    *   **Known weak spot**: the sanity report matches spends to drops greedily, closest to the expected time first. In bursts of identical spends (10 houses in 4 s at 30:00 in the sample game), a sync a few ms off can flip one match. The sample game scores 188/189, and 187/189 is equally plausible for a correct sync. Matching bursts in order would remove the flip.
*   **Cancelled Final Age-up Click**: Only the last click per age counts as the age-up (earlier clicks were cancelled). If that last click was cancelled too and never repeated, the age-up is still counted. Detecting it would mean matching a queue cancellation at the Town Center. Game 1 has a real case: Imperial clicked at 29:15.100 and cancelled (the replay records a TC DeQueue at position 0 at 29:27.885), yet the report marks Imperial at 29:15; its analysis snapshot records this. Tracking the TC queue through DeQueue commands would fix it.
*   **Game Data per Replay Build (later)**: `data/game_data.csv` holds the costs of one game version (see `data/SOURCES.md`). Older replays are read with the current costs, which is wrong for units whose cost changed since. Only worth doing if old replays matter; the generator script can produce one table per build.
*   **Civilization Bonuses**: costs are base costs; civilization discounts (e.g. cheaper units or techs for one civilization) are not applied, so their spends show as unmatched or over-charged in the sanity report.
*   **Typed Readings at the Capture Boundary**: The analysis works on typed observations, but capture still writes the telemetry CSV straight from the vision pipeline's string map (`Results`), and the frame recorder parses that map again (`PanelValues`). A typed panel reading would remove the duplication.
*   **Duplicated Age Names**: `constants::GAME_AGES` duplicates the names of the `Age` enum.
*   **Confidence-Based Rejection in Vision**: The matcher always returns its best guess, so a doubtful digit becomes a confidently wrong number. Report low-confidence reads as unread instead. Sync now skips unread values, so this is no longer blocked.
*   **Vision CPU Cost**: The live test measured 25–38 ms per frame, about 10% of one core at 4 frames per second. The anchor search, calibration and the main pass each convert the whole frame to RGB again (`to_rgb8`, about 6 MB at 1080p), although only a strip at the top is used. Profile it, then crop the strip once before any processing. In game 1 (a laptop running the game, Squire and video encoding) the console's per-tick `Time:` was 70–165 ms live at 4 FPS, against 27 ms for the same frames replayed with `--replay-frames`; the live figure may include waiting for the CPU under load, but if it is CPU time it is far above the <1% target. The game itself ran about 2% slow (sync speed 1.665 instead of 1.7).
*   **Sanity Report on Every Recorded Game**: `tests/sanity_report.rs` checks the sample game and game 1, each against its `test_bench/snapshots/<game>.sanity.json`. Add each further recorded game the same way.

