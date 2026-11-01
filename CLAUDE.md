# AoE2 Squire — Project Context

## What We're Building
A native Windows background tool that scrapes AoE2:DE resource/villager counts from the screen in real time and combines them with replay data to produce post-game efficiency coaching. Target: minimal CPU footprint (<1% to avoid impacting the game), no memory hooking, no anti-cheat risk.

## Current Status

- Python R&D (`research/`) is finished and remains the canonical reference.
- The Rust production binary is fully functional, with a complete vision pipeline operating under 1% CPU overhead.
- Master Orchestrator is operational, automating capture, replay discovery, data meshing, and analysis.
- Interactive, offline-capable HTML reports are automatically generated and bundled with Chart.js for standalone use.
- The capture loop reads frames from a `FrameSource` (live DXGI capture or a folder of recorded PNGs), so it is tested without a game. `--record-frames` saves masked resource-panel frames of a match next to its telemetry for checking it afterwards.

## Repo Layout
```
aoe2-squire/
├── CLAUDE.md               ← you are here
├── DECISION_LOG.md         ← all architectural decisions with rationale — read this
├── TECHNICAL_SPEC.md       ← product-level requirements
├── Cargo.toml              ← Rust manifest
├── ui_map.json             ← element coordinates (baseline 1080p, scaled at runtime)
├── src/
│   ├── main.rs             ← entry point
│   ├── lib.rs              ← module definitions
│   ├── constants.rs        ← all pipeline constants
│   ├── analysis/           ← modular analyzer implementation
│   ├── capture/            ← capture loop, frame sources (DXGI, recorded PNGs), frame recording
│   ├── pipeline/           ← Vision pipeline implementation
│   ├── bin/                ← Standalone CLI tools
│   ├── replay.rs           ← replay parsing logic
│   ├── replay_discovery.rs ← automated replay location
│   ├── report.rs           ← HTML report generator
│   ├── sync/               ← replay/telemetry time mapping (spend↔drop alignment, piecewise fit)
│   ├── types.rs            ← shared data structures
│   └── validation.rs       ← sanity checks for a captured game (no ground truth needed)
├── templates/              ← HTML report templates
├── tests/                  ← analysis snapshot, sync stress, vision fixtures, sanity report, capture loop
├── tools/                  ← scripts (video frame fixture extraction, video/telemetry alignment)
├── test_bench/             ← reference screenshots, sample game, fixtures, snapshots
└── research/               ← Python R&D only (reference)
```

The replay parser `aoe2rec` is our fork (github.com/chris-h-sg/aoe2rec), pinned by commit in `Cargo.toml`. Parser changes go to the fork first, then bump `rev`.

## Validating a Change
Check that a change works without playing a game, in this order:
1. `cargo test`: analysis snapshot (`UPDATE_SNAPSHOTS=1` regenerates after an intended change), sync stress tests (the sample game distorted by speed, pauses, varying speed, unread values and a late start; `-- --nocapture` prints each scenario's errors; `#[ignore]`d ones would be known bugs), vision fixtures with an accuracy baseline, the sanity report on the sample game, and the capture loop (game start/end, telemetry rows, housing interpolation, frame recording) on recorded video frames.
2. `cargo run --bin sanity_report -- <telemetry.csv> [replay.aoe2record] [--json]` on any captured game: spend alignment (replay spends vs resource drops on screen; ~99% on the sample game; sync is fitted to maximise these matches, so this is not an independent check of sync and only drops for sync errors larger than the match window of a few seconds), single-frame glitches, invariant violations, unexplained drops and increases.
3. Spot-check frames where they exist: crop and enlarge the resource panel, read it, and compare with the CSV. A game played with `--record-frames <dir>` keeps masked frames in `<dir>/telemetry_<start>/`. They are saved every 5 s, on sharp jumps (with the frame before) and when the population starts or stops blinking. `frames.csv` gives each frame's `timestamp_ms`, which is its CSV row's timestamp and the `rw` timestamp the sanity report prints, so a flagged moment can be looked up in the nearest saved frame. `--replay-frames <dir>/telemetry_<start>` runs a recording back through the capture loop; after a vision change, compare its CSV with the original rows. Where a game has a video (`--record-video <dir>` in a `--features dev-tools` build, or a Steam recording), `python tools/align_video.py <video> <csv> --at <rw>` gives the video time of a flagged moment. See `test_bench/README.md`.
4. Flag anything inexplicable rather than guessing. Known legitimate causes of unexplained drops: automatic farm reseeding, failed placements, walls, market and tributes (not parsed from the replay).
