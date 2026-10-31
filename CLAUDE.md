# AoE2 Squire — Project Context

## What We're Building
A native Windows background tool that scrapes AoE2:DE resource/villager counts from the screen in real time and combines them with replay data to produce post-game efficiency coaching. Target: minimal CPU footprint (<1% to avoid impacting the game), no memory hooking, no anti-cheat risk.

## Current Status

- Python R&D (`research/`) is finished and remains the canonical reference.
- The Rust production binary is fully functional, with a complete vision pipeline operating under 1% CPU overhead.
- Master Orchestrator is operational, automating capture, replay discovery, data meshing, and analysis.
- Interactive, offline-capable HTML reports are automatically generated and bundled with Chart.js for standalone use.

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
│   ├── capture/            ← DXGI screen capture implementation
│   ├── pipeline/           ← Vision pipeline implementation
│   ├── bin/                ← Standalone CLI tools
│   ├── replay.rs           ← replay parsing logic
│   ├── replay_discovery.rs ← automated replay location
│   ├── report.rs           ← HTML report generator
│   ├── sync.rs             ← temporal calibration/sync
│   ├── types.rs            ← shared data structures
│   └── validation.rs       ← sanity checks for a captured game (no ground truth needed)
├── templates/              ← HTML report templates
├── tests/                  ← analysis snapshot, sync stress, vision fixtures, sanity report
├── tools/                  ← one-off scripts (video frame fixture extraction)
├── test_bench/             ← reference screenshots, sample game, fixtures, snapshots
└── research/               ← Python R&D only (reference)
```

The replay parser `aoe2rec` is our fork (github.com/chris-h-sg/aoe2rec), pinned by commit in `Cargo.toml`. Parser changes go to the fork first, then bump `rev`.

## Validating a Change
Check that a change works without playing a game, in this order:
1. `cargo test`: analysis snapshot (`UPDATE_SNAPSHOTS=1` regenerates after an intended change), sync stress tests (`#[ignore]`d ones are known bugs; `-- --ignored` runs them), vision fixtures with an accuracy baseline, and the sanity report on the sample game.
2. `cargo run --bin sanity_report -- <telemetry.csv> [replay.aoe2record] [--json]` on any captured game: spend alignment (replay spends vs resource drops on screen; ~99% on the sample game, far lower when sync is broken), single-frame glitches, invariant violations, unexplained drops and increases.
3. Spot-check frames where they exist: crop and enlarge the resource panel, read it, and compare with the CSV.
4. Flag anything inexplicable rather than guessing. Known legitimate causes of unexplained drops: automatic farm reseeding, failed placements, walls, market and tributes (not parsed from the replay).
