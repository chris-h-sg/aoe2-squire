# test_bench

Reference data for the automated tests (`cargo test`, no game needed):

| Data | Used by |
|---|---|
| Screenshots + `expected_values.json` | `tests/vision_fixtures.rs` |
| `video_frames/` (masked frames from the `.mp4` clips, see `tools/extract_video_fixtures.py`) | `tests/vision_fixtures.rs`, `tests/capture_loop.rs` |
| `1v1-RM-sample.csv` + `.aoe2record` | `tests/analysis_snapshot.rs`, `tests/sync_stress.rs`, `tests/sanity_report.rs` |
| `game1-castle-vs-ai.csv` + `.aoe2record` (recorded sample game 1, see below) | `tests/analysis_snapshot.rs`, `tests/sanity_report.rs` |
| `game_frames/<game>/` (selected frames of a recorded game) | `tests/vision_fixtures.rs` |
| `interpolation/` | housing interpolation unit tests |
| `snapshots/` | regression baselines (`UPDATE_SNAPSHOTS=1` regenerates them), and each game's sanity report findings with explanations (`<game>.sanity.json`) |

Videos and full frame sets are too large for this repository. They are release assets of
[chris-h-sg/aoe2-squire-data](https://github.com/chris-h-sg/aoe2-squire-data), one release
per recording batch:

```bash
gh release download sample-games-2026-11 -R chris-h-sg/aoe2-squire-data
```

## Recorded sample games

| Game | Files | Data release | Notes |
|---|---|---|---|
| Game 1 (2026-11-01) | `game1-castle-vs-ai.*`, `game_frames/game1-castle-vs-ai/`, `snapshots/game1-castle-vs-ai.*` | [`sample-games-2026-11`](https://github.com/chris-h-sg/aoe2-squire-data/releases/tag/sample-games-2026-11): `telemetry_20261101_110257.mkv`, `.video.json`, frames zip | Live vs AI (Emperor Leo VI), Thrar (Britons), QS Arabia, Normal speed, 1920x1080, 100% UI, default font; AI resigned at 29:40. Sync speed 1.665 in 5 segments, 138/141 spends matched. |

Game 1 against the checklist below: Feudal and Castle clicked and cancelled, Imperial
clicked and cancelled at the end (counted as an age-up, a known limitation); multi-range
archer queues (single clicks and a shift batch); militia queued and cancelled; two Market
foundations placed and deleted; housed for about 36 s right after the Castle click, and
queue-blocked often; boar hunters dropping off together; a palisade wall; 10 farm reseeds,
none pre-queued at the mill. Not covered: 5-digit resources, 100+ villagers and population
150+ (the game ended at 61 population), stone walls, market buy and sell, repairs,
pre-queued reseeds. The frame-level timestamps of the 15-frame `pop_vils` 111 misread at
00:35 are in `snapshots/game1-castle-vs-ai.sanity.json`. Open question: the extra Feudal
clicks left no trace in the replay or on screen (see ROADMAP.md).

## Sample games to record

The first full game (`1v1-RM-sample`) was captured from a replay playing back, at
normal speed, with no pauses and one click per Age-up. These 2–3 games cover what the
tests can only simulate today (game 1 is recorded, see above).

**Every game:** record with `--record-frames` and `--record-video` (see below; a Steam
recording works too), keep the `.aoe2record`, and turn the in-game
clock on (F11) for the whole game. Play live, not from a replay: only a live game runs at
the true game speed. Note the rough in-game time of each deliberate scenario below; that
makes the sanity report's findings quick to explain.

**Game 1: Normal speed (1.7), economy and edge cases**
- Age-ups: click Feudal, cancel, click again; the same for Castle. Late in the game, click an
  Age-up, cancel it and do not click again (known limitation: the last click counts as an
  age-up).
- Queues with 2–3 buildings selected (TCs, stables, ranges), single clicks and one
  shift-click batch of 5.
- Refunds: cancel queued units, and delete a foundation before it is started.
- Housing: stay housed for 10+ s; be queue-blocked (yellow population) once; let a house
  finish while housed.
- Idle villagers: leave a few idle for 20+ s, at least twice.
- Floating: bank 500+ food or wood in Feudal for 30+ s; build a Castle (stone save-up).
- Spends the replay parser does not read: palisade and stone walls, market buy and sell,
  repairing a building, farm reseeds both pre-queued at the mill and manual.
- Several hunters on a boar or sheep at the TC (simultaneous drop-offs).
- Play into late Castle or Imperial Age so numbers get large: 5-digit resources, 100+
  villagers, population 150+.
- End with a resignation (match end and winner detection).

**Game 2: another game speed (Casual 1.5× or Fast 2.0×), timing**
- Pause twice: about 10 s between the first spend and the Feudal click, and about 40 s in
  Castle Age.
- Start Squire 10–20 s after the game has started (late capture start, after the first
  spends).
- A couple of repeated Age-up clicks at this speed too.
- In the last minute only: open the in-game menu and alt-tab out briefly. Capture currently
  ends the match on the first frame without the resource panel, so this would cut the
  recording short if done earlier.

**Game 3: replay playback and vision variety (optional)**
- Capture one of the games above while playing back its replay, and change the playback
  speed midway (e.g. 1× → 2× → 1×) and pause it once. Replay playback has its own speed,
  which is not the game speed and can drift, so this tests sync with a varying speed.
- If possible, a different resolution or in-game UI size, and the Anne_HK mod if you use it.

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

**Video (dev builds):** `--record-video <dir>` records the screen with ffmpeg for the
length of each match. It is behind the `dev-tools` Cargo feature, so release builds do not
have it. It needs ffmpeg 6.0 or newer (`winget install Gyan.FFmpeg`; update it with
`winget upgrade Gyan.FFmpeg`):

```bash
cargo run --release --features dev-tools -- --record-frames recordings --record-video recordings
```

- `recordings/telemetry_<start>.mkv`: the primary monitor (the one Squire reads) at 30 fps,
  H.264. Matroska stays playable if Squire is killed (remux it with
  `ffmpeg -i in.mkv -c copy out.mkv` to make it seekable again).
- `telemetry_<start>.video.json`: when ffmpeg was started, the match start, the encoder and
  the full ffmpeg command. `telemetry_<start>.ffmpeg.log`: ffmpeg's warnings and errors.
- ffmpeg starts when the match is detected and gets `q` when the match UI disappears, when
  Squire is stopped with Ctrl+C, and when Squire panics (before the error message waits
  for Enter). If Squire is killed or its window is closed, Windows kills ffmpeg. The first
  video frame comes about 0.6 s after the match was detected.
- Options: `--ffmpeg <path>` (or `SQUIRE_FFMPEG`; default `ffmpeg` on PATH),
  `--video-encoder nvenc|amf|qsv|x264` and `--video-fps <n>`. At startup Squire checks
  that ffmpeg has the `ddagrab` screen capture filter and tries the GPU encoders in that
  order with a short test capture. x264 is the CPU fallback.
- The encoder must be on the GPU that drives the screen, because that is where the captured
  frames are. On laptops with hybrid graphics (NVIDIA Optimus) the built-in screen is wired
  to the integrated Intel GPU, even when the game renders on the NVIDIA card, so QuickSync
  is the encoder there. That is normal and leaves the NVIDIA card to the game. NVENC also
  needs a driver recent enough for the ffmpeg build (ffmpeg 9 needs 610+); Pascal cards
  such as the GTX 1050 Ti stay on the 580 driver series, which is too old for it.
- Measured on a laptop (i7, GTX 1050 Ti, built-in 1080p screen on the Intel HD 630,
  QuickSync): ffmpeg used about 13% of one core, the video ran at 29.9 fps (a few
  single-frame gaps, plus about 0.3 s at the start), and took 30-35 MB per minute in a short
  test, but about 56 MB per minute over game 1 (995 MB for 18 minutes real time).

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
- Find a moment in the video. `tools/align_video.py` reads the resource panel from video
  frames with the release binary (`cargo build --release` first) and matches it against
  the CSV. It prints the offset, checks it in four parts of the video for drift, and
  converts CSV `timestamp_ms` / `rw` values or times since the match start to video times:

  ```bash
  python tools/align_video.py recordings/telemetry_<start>.mkv logs/telemetry_<start>.csv --at <rw> --at 12:34
  ```

  It works for Steam recordings too, which start before the match. A 5-minute video takes
  about a minute, a 20-minute one about two; the offset comes out to within about
  ±50 ms. `--offset-ms` skips measuring and only converts.
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
  `test_bench/snapshots/<game>.sanity.json` like `1v1-RM-sample.sanity.json`, and add a
  test like `game1_findings_are_all_reviewed` in `tests/sanity_report.rs`.
  `UPDATE_SNAPSHOTS=1 cargo test --test sanity_report` writes the findings and keeps
  existing explanations; the test fails until every finding has one.
- **Data release**: upload the video, its `.video.json` and a zip of the frame folder
  (without `replayed/`) as assets of a release in `chris-h-sg/aoe2-squire-data`, and add
  the game to "Recorded sample games" above.
- **Capture loop**: a recorded frames folder can be fed to the capture loop in a test with
  `ImageSequenceSource::from_dir` (see `tests/capture_loop.rs`).
