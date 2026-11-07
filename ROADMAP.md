# AoE2 Squire Roadmap

This roadmap outlines the development path for the AoE2 Squire, prioritizing core metrics and automation to provide performance data to players, before expanding into complex computer vision features.

## Next Steps (planned 2026-11-02)

In this order; each step is its own piece of work (and session), tracked against this list:

1. **Digit matcher replacement** (see "Digit Recognition Review" below): the measurements show matching is both the main CPU cost and, through splitting, the main cause of misreads, so one change addresses both. It includes cropping the panel strip once (see "Vision CPU Cost"). **In progress:** the learned-shape matcher runs end to end behind `--matcher learned` (DECISION_LOG.md, "Learned Glyph Shapes"); the split matcher stays the default. Status (2026-11-07), fixture fields correct / misread (wrong value) / unread:

    | Fixture group | Split matcher | Learned, before the accuracy fixes | Learned matcher now |
    |---|---|---|---|
    | Screenshots 1080p | 212 / 12 / 0 | 204 / 5 / 18 | 220 / 1 / 3 |
    | Screenshots 1366x768 | 14 / 0 / 0 | 14 / 0 / 0 | 14 / 0 / 0 |
    | Video stills | 625 / 1 / 0 | 580 / 6 / 40 | 622 / 0 / 4 |
    | Game 1 | 107 / 2 / 0 | 108 / 0 / 1 | 109 / 0 / 0 |
    | Game 2 | 67 / 0 / 0 | 66 / 0 / 1 | 67 / 0 / 0 |
    | Game 3 (75% UI) | 37 / 19 / 0 | 54 / 0 / 2 | 56 / 0 / 0 |
    | Total (incl. out of scope) | 1118 / 34 / 0 | 1060 / 15 / 80 | 1132 / 1 / 19 |
    | Out of scope: 1200p/1440p screenshots (old, from the web) | 56 / 0 / 0 | 34 / 4 / 18 | 44 / 0 / 12 |
    | Time per 1080p frame (`vision_bench`) | 22.8 ms (23.5 ms on 2026-11-03) | 52 ms (167 ms on the first run) | 20.9 ms (63 ms on 2026-11-03) |

    Since this table was first made, the three enormous-font screenshots are reference-only (no player sees that layout in a game, so they are no longer tested; they were 3 misreads for both matchers), and `extract_f0302`'s population colour accepts white or yellow (the frame is caught mid-fade, ink about 242,239,217). The middle column predates both changes.

    On the recorded frames of games 1 and 2 (`--replay-frames`) both matchers now give identical CSVs. On game 3 every disagreement checked by eye is a split-matcher misread (70 read as 711, 0 as 11, 6 as 48, 9 as 41, 30 as 311), and the learned matcher leaves nothing unread there. The sanity report without the replay finds 0 glitches and 0 invariant violations on game 3, against 2 and 7 for the split matcher.

    Weak spots before it can become the default:
    *   **Video stills**: the population villager count over its icon art (36, 60) is unread in 2 frames. Compression lifts the black outline above the outlined-text mask's threshold.
    *   **Anne_HK**: food villagers 42 read as 12 in `7_vil_Anne_HK-2` (the only in-scope wrong value), and the idle count 103 is unread. The Anne_HK shapes come from two screenshots.
    *   **Sans-serif fixtures**: the population total of `aoe2_16x9_min_sans` is unread (the split matcher misreads it).
    *   ~~**CPU**~~ (done 2026-11-07): 20.9 ms per 1080p frame against the split matcher's 22.8 ms (same run), down from 63 ms, with every fixture reading unchanged. The `--dump` of `vision_bench` matched on all 1056 fields after each step. In stages: observation 5.1 -> 1.4 ms (sliding-histogram percentile, inpainting without copies), pen height 11.4 -> 2.4 (gains vectorised over rows of pens; the idle count skipped while its icon shows no idle villagers), chains 10.6 -> 4.6 (no per-cell allocations), re-scoring 16.8 -> 8.0 and confidence 13.6 -> 4.0 (a glyph's alternatives costed against the canvas without it; refinement re-tries only glyphs a move reaches; no confidence work once a field is doubtful). The population total remains the dearest field (5.7 ms). In the capture loop a field whose pixels are unchanged keeps its last reading: on the recorded frames of game `telemetry_20261101_110257` (302 frames, ~5 s apart) 17.9 -> 11.5 ms per frame, with identical results. At 4 FPS more fields are unchanged between frames, so live it should be lower still (not measured).
    *   **Higher resolutions**: to be measured on fresh screenshots; the old ones from the web are out of scope.
2. **Quick replay fixes**: the cancelled final Age-up click and in-order matching of bursts of identical spends (both under "Technical Debt" below). Independent of 1.
3. **Survive a short loss of the panel** (below). Touches the capture loop; independent of 1.
4. **Sync phase 2 remainder**: explain game 1's 5 segments, then record a regular-speed replay playback at 1x/2x/1x (game 3 has no speed change within the supported 0.5-4.0).
5. **Upstream parser fix**: decide whether to offer the Fast-speed AI fix (`fix/ai-skip-fast-speed` in chris-h-sg/aoe2rec) to liouh/aoe2rec.
6. **Release preparation**: user-facing README limits (regular-speed replay playback only; UI scales the matcher still misreads; pauses are now tested on real games).

Later: fast replay playback analysis (needs 1 and a higher capture rate), civilization cost bonuses, refreshing civ/map data from aoc-reference-data, game data per replay build, and a 4th sample game for what games 1-3 did not cover (5-digit resources, 100+ villagers, population 150+, Imperial Age, stone walls, market buys, other resolutions, Anne_HK live).

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
*   **Digit Recognition Review (after the next recorded games):** The pipeline locates the panel, filters text colours, splits each number into single characters (connected components plus recursive valley splitting for touching digits), then matches each character against one template set scaled down from the large "enormous" font. For one known font this approach is sound: about 1 wrong field in 900 on the supported fixtures, and a live capture matched an earlier one on 99.97% of readings. The likely local maximum is the split-first structure: each splitting heuristic patches one case and risks another, and scaled-down templates don't match how the game draws small numbers. Game 3 adds the 75% UI scale, where mostly a 0 is read as two digits (70 as 711, 50 as 511; `test_bench/game_frames/game3-playback-75ui/`, known failures). Game 1 adds a reproducible case: the villager count 10, drawn over the edge of the population icon, is read as 111 (`test_bench/game_frames/game1-castle-vs-ai/`, known failures).
    *   **Decide with data:** run the vision accuracy baseline and the sanity report on the new recordings (other resolutions and UI sizes, large late-game numbers). If misreads cluster in splitting or in particular sizes, build a replacement side by side and switch only if it beats the current pipeline on all fixtures. If not, keep fixing individual cases.
    *   **Measured (2026-11-02, decision: build the replacement):** of the 37 failing fixture fields (1118/1155 correct), 18 are splitting errors in the default font: at 75% UI scale digits are about 7 px tall with 1-px strokes, the top and bottom of a 0 (or 6) fall below the filter threshold and its two sides become separate pieces (0 read as 11 or 41, 6 as 48); game 1's 10 read as 111 is the same kind. 3 more are template mismatches of tiny pieces scaled up to 64x64 (9 as 0, 4 as 1), 12 are the unsupported sans-serif font, and 4 are not digits (3 enormous-font layouts detected as a game, 1 colour after the housed overlay). Matching is also the main CPU cost (see "Vision CPU Cost"), so the replacement must beat the current pipeline on every fixture **and** cost less CPU per frame.
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
*   **Record Sample Games:** 2–3 games covering game speeds, pauses, repeated Age-up clicks, refunds and more, following the checklist in `test_bench/README.md` ("Sample games to record"). They become fixtures for the sync rework and the sanity report, and the evidence for the Digit Recognition Review. Games 1-3 are recorded (`test_bench/game1-castle-vs-ai.*`, `test_bench/game2-fast-pauses.*`, `test_bench/game3-playback-75ui.csv`, videos and frames in the data release; see `test_bench/README.md`).

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
*   **Analyse a Replay by Fast Playback (later)**: play a recorded game back at 4x or 8x and capture it, so a report takes minutes instead of the length of the game. Today the sync searches speeds up to 4.0 (regular playback of a Normal or Fast game), and at 8x playback the 4 captures per second are 4 s of game time apart, too coarse to tell spends apart. Needs a higher capture rate (and its CPU cost) as well as a higher `MAX_SPEED`. `test_bench/game3-playback-75ui.csv` is a recording of this (2.0, 8.0 and 16.0, with a pause).
*   **Game Data per Replay Build (later)**: `data/game_data.csv` holds the costs of one game version (see `data/SOURCES.md`). Older replays are read with the current costs, which is wrong for units whose cost changed since. Only worth doing if old replays matter; the generator script can produce one table per build.
*   **Civilization Bonuses**: costs are base costs; civilization discounts (e.g. cheaper units or techs for one civilization) are not applied, so their spends show as unmatched or over-charged in the sanity report.
*   **Typed Readings at the Capture Boundary**: The analysis works on typed observations, but capture still writes the telemetry CSV straight from the vision pipeline's string map (`Results`), and the frame recorder parses that map again (`PanelValues`). A typed panel reading would remove the duplication.
*   **Duplicated Age Names**: `constants::GAME_AGES` duplicates the names of the `Age` enum.
*   **Confidence-Based Rejection in Vision**: The matcher always returns its best guess, so a doubtful digit becomes a confidently wrong number. Report low-confidence reads as unread instead. Sync now skips unread values, so this is no longer blocked.
*   **Vision CPU Cost**: The live test measured 25–38 ms per frame, about 10% of one core at 4 frames per second. The anchor search, calibration and the main pass each convert the whole frame to RGB again (`to_rgb8`, about 6 MB at 1080p), although only a strip at the top is used. Profile it, then crop the strip once before any processing. In game 1 (a laptop running the game, Squire and video encoding) the console's per-tick `Time:` was 70–165 ms live at 4 FPS, against 27 ms for the same frames replayed with `--replay-frames`; the live figure may include waiting for the CPU under load, but if it is CPU time it is far above the <1% target. The game itself ran about 2% slow (sync speed 1.665 instead of 1.7).
    *   **Measured per stage (2026-11-02, 91 detected 1080p fixture frames, release build):** `process_frame` 27.2 ms, of which digit matching 18.5 ms (68%; about 25 digits per frame at 0.73 ms each: 11 templates x 9 shifts on a 64x64 canvas), the three full-frame `to_rgb8` copies 7.8 ms (29%), and splitting, filtering and the anchor scan together under 1 ms. Live capture adds the full-frame `bgra_to_rgb_image` (4.4 ms, per-pixel `put_pixel`), which the console's `Time:` does not include. In total about 31.5 ms per tick, about 13% of one core at 4 FPS. At 5120x1440 the pipeline takes 47 ms: the copies grow with the screen, matching does not. So cropping alone saves under a third; native-size matching (see "Digit Recognition Review") goes after the rest.
*   **Sanity Report on Every Recorded Game**: `tests/sanity_report.rs` checks the sample game, game 1 and game 2, each against its `test_bench/snapshots/<game>.sanity.json` (game 3 is checked to be refused). Add each further recorded game the same way.
*   **Short Loss of the Panel Ends the Match**: the capture loop ends the match on the first frame without the resource panel (`src/capture/mod.rs`, "Match UI lost"). Game 2 lost its last 3.5 minutes to a brief alt-tab. End the match only after the panel has been gone for a while, or when the game has really ended.
*   **Game 1 Synced in 5 Segments**: a live game at constant speed without pauses should give one segment (speed 1.665). Find out what splits it.

