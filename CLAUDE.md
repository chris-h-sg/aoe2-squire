# AoE2 Squire — Project Context

## What We're Building
A native Windows background tool that scrapes AoE2:DE resource/villager counts from the screen in real time and combines them with replay data to produce post-game efficiency coaching. Target: minimal CPU footprint (<1% to avoid impacting the game), no memory hooking, no anti-cheat risk.

## Current Status
**Phase 5 Complete — Integrated Reporting & Prototype Release.**

- Python R&D (`research/`) is finished and remains the canonical reference.
- The Rust production binary is fully functional, with a complete vision pipeline operating under 1% CPU overhead.
- Master Orchestrator is operational, automating capture, replay discovery, data meshing, and analysis.
- **Reporting:** Interactive, offline-capable HTML reports are automatically generated and bundled with Chart.js for standalone use.

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
│   ├── constants.rs        ← all pipeline constants
│   ├── pipeline/           ← Vision pipeline implementation
│   ├── capture/            ← DXGI screen capture implementation
│   ├── bin/                ← Standalone CLI tools
│   ├── analysis.rs         ← shared metrics helpers
│   └── report.rs           ← HTML report generator
├── templates/              ← HTML report templates
├── test_bench/             ← reference screenshots
└── research/               ← Python R&D only (reference)
```
