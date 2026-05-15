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
│   └── types.rs            ← shared data structures
├── templates/              ← HTML report templates
├── test_bench/             ← reference screenshots
└── research/               ← Python R&D only (reference)
```
