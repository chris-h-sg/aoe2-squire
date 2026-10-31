# test_bench

Reference data for the automated tests (`cargo test`, no game needed):

| Data | Used by |
|---|---|
| Screenshots + `expected_values.json` | `tests/vision_fixtures.rs` |
| `video_frames/` (masked frames from the `.mp4` clips, see `tools/extract_video_fixtures.py`) | `tests/vision_fixtures.rs`, `tests/capture_loop.rs` |
| `1v1-RM-sample.csv` + `.aoe2record` | `tests/analysis_snapshot.rs`, `tests/sync_stress.rs`, `tests/sanity_report.rs` |
| `interpolation/` | housing interpolation unit tests |
| `snapshots/` | regression baselines (`UPDATE_SNAPSHOTS=1` regenerates them), and the sample game's sanity report findings with explanations (`1v1-RM-sample.sanity.json`) |

## Recording a game and turning it into fixtures

A recorded game gives three things that belong together: the telemetry CSV, masked frames
of the resource panel with their timestamps, and the replay. The frames make it possible to
check the readings frame by frame after the game, and to build new vision fixtures.

### 1. Record

```bash
cargo run --release -- --record-frames recordings
```

Play a match as usual. On top of `logs/telemetry_<start>.csv`, this saves
`recordings/telemetry_<start>/`:

- `<timestamp_ms>.png`: full-size frames, black except for the resource panel and the
  anchor band (same masking as `tools/extract_video_fixtures.py`; the readings are
  identical to the full frame).
- `frames.csv`: `timestamp_ms,file,reason`. `timestamp_ms` equals the `timestamp_ms` of the
  frame's CSV row. Reasons: `first`, `interval` (every 5 s), `change:<column>` (a total
  moved by 150+ or a count by 3+ in one tick, or a field appeared or disappeared),
  `before` (the frame before a jump, to tell a misread from a real change),
  `pop_color:start` / `pop_color:end` (the population number starts blinking yellow or
  with the housed overlay, or has stopped for 2 s) and `end` (the frame on which the
  match UI was lost; it has no CSV row).

The console prints the replay path when the match ends (`[Discovery] ...`); it is in
`%USERPROFILE%\Games\Age of Empires 2 DE\<profile id>\savegame\`.

Cost: about 3 ms of CPU per saved frame (1080p), on a below-normal-priority thread, and
16-18 frames per minute (estimated from a 5-minute live test and the 20-minute sample
game), so well under 0.1% of one core. On disk, 200-240 KB per frame at 1080p and 400 KB
at 1440p, so roughly 3-7 MB per minute.
Frames are dropped, never waited for, if the disk cannot keep up.

### 2. Check the recording

- Re-read the frames through the capture loop; the replayed rows must equal the original
  rows with the same `timestamp_ms` (the `housing` column may differ, as it is interpolated
  from many fewer frames). After a vision change, the differences show its effect.

  ```bash
  cargo run --release -- --replay-frames recordings/telemetry_<start>
  ```

  This writes `recordings/telemetry_<start>/replayed/telemetry_*.csv`.
- Run the sanity report on the new CSV and replay:

  ```bash
  cargo run --bin sanity_report -- logs/telemetry_<start>.csv path/to/match.aoe2record
  ```

  It prints each finding with its `rw` timestamp, which is the `timestamp_ms` of the CSV
  row and of the saved frame, so the nearest frame in `frames.csv` shows what the panel
  read at that moment.
- Look at the `change:` and `before` frames: each pair is either a real change (a spend,
  a house finished) or a misread. Read the panel in the PNG and compare with the CSV row.

### 3. Turn it into fixtures

Keep only selected frames in the repository (a whole recording is several MB per minute).

- **Vision fixtures**: copy the interesting frames (misreads, unusual layouts, a few
  ordinary ones) to a new folder such as `test_bench/game_frames/<game>/` and write its
  `expected_values.json` in the format of `video_frames/expected_values.json`. Take the
  expected values from reading the panel in each image, not from the CSV: the CSV is what
  the pipeline read, which is what is being tested. List fields that are wrong today
  under `known_failures`. Add the manifest to `MANIFESTS` in `tests/vision_fixtures.rs`,
  then raise the accuracy baseline with
  `UPDATE_SNAPSHOTS=1 cargo test --test vision_fixtures`.
- **Analysis snapshot**: copy the CSV and the replay to `test_bench/<game>.csv` and
  `test_bench/<game>.aoe2record`, add a test next to `analysis_snapshot_1v1_rm_sample` in
  `tests/analysis_snapshot.rs` with its own snapshot file, and create it with
  `UPDATE_SNAPSHOTS=1 cargo test --test analysis_snapshot`. Review the JSON (game start,
  speed, age-up times) before committing: it is a regression baseline, not ground truth.
- **Sanity report findings**: explain every finding of the new game (farm reseed, failed
  placement, misread, ... checked in the saved frames or on video) in a
  `test_bench/snapshots/<game>.sanity.json` like `1v1-RM-sample.sanity.json`.
  `tests/sanity_report.rs` reads only the sample game's file today (`REVIEWED_FINDINGS`),
  so extend it to a list of games. `UPDATE_SNAPSHOTS=1 cargo test
  --test sanity_report` writes the findings and keeps existing explanations; the test fails
  until every finding has one.
- **Capture loop**: a recorded frames folder can be fed to the capture loop in a test with
  `ImageSequenceSource::from_dir` (see `tests/capture_loop.rs`).
