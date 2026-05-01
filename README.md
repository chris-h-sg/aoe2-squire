# Project: RTS Analyzer

## Vision
A cross-game RTS coaching tool that provides **instant, narrative-driven feedback** immediately after a match ends. Unlike current tools that require manual uploads or watching replays, RTS Analyzer uses a local agent to provide a "Post-Game Report" the second the match is over.

## The Core Concept
The "Hybrid Data Strategy":
- **Screen Scraping:** Captures the "Current State" (resources, idle counts, villager allocation) during the game without touching memory.
- **Replay Parsing:** Captures the "Deterministic Log" (tech timings, unit production) from the local `.aoe2record` or similar file as soon as the game finishes.
- **Analysis:** Combines both to identify "Efficiency Gaps" (e.g., "You had the tech for faster gathering but your resource intake didn't rise, implying poor lumber camp placement").

## Project Status
- **Phase:** Validation & Sync (Phase 4).
- **Primary Goal:** Align live vision telemetry with ground-truth replay data to create a high-precision game analysis stream.
- **Status:** The `mesher` tool is fully operational, providing sub-second temporal alignment using a two-point linear calibration model. The pipeline successfully extracts and merges unit production, tech research, and building events with live resource and population telemetry.

## Development
The production pipeline is written in Rust. The Python code in `/research` is strictly for rapid prototyping and validation, and is not part of the shipped product.

### Running the Tests
To run the vision pipeline integration test suite (validates digit recognition against all test screenshots):
```powershell
cargo test
```

To run the tests and show the pipeline's print output (e.g., detected UI scale and overlay detections):
```powershell
cargo test -- --nocapture
```

### Running the Extractor

By default, the application runs in **Live Capture Mode**. It uses Windows DXGI (Desktop Duplication) to capture your primary monitor every 500ms and stream real-time telemetry to the console.

### Telemetry Logging
While running in live mode, the application automatically persists telemetry data to timestamped CSV files in the `logs/` directory (e.g., `logs/telemetry_20260429_150000.csv`). This data is used for offline analysis and validation against game replays.

```powershell
# Run the live capture loop (highly recommended to use --release for performance)
cargo run --release
```

#### Static Image Mode
You can still process a specific static image file by passing its path as an argument:

```powershell
# Example:
cargo run -- test_bench/aoe2_4k.png
```

To explicitly force live mode via flag:
```powershell
cargo run -- --live
```

### Replay Parsing

The analyzer can parse Age of Empires II: DE replay files (`.aoe2record`) to extract ground-truth gameplay events. This is used to validate the vision pipeline's accuracy.

```powershell
# Extract all game events (techs, units, buildings) from a replay
cargo run -- --parse-replay "path/to/your/match.aoe2record"
```

The tool currently extracts:
*   **Player Metadata**: Names, civilizations, and color assignments.
*   **Technology Research**: Accurate `mm:ss.sss` timestamps and resource costs for every technology started.
*   **Unit Training & Queuing**: Tracking of unit production, queuing, and cancellations with specific Building ID attribution.
*   **Building Construction**: Complete timeline of building foundations and completions.
*   **Deletion Events**: Tracking of unit and building deletions/deaths for accurate population reconciliation.

### Data Synchronization & Meshing

The `mesher` utility is the heart of the validation pipeline. it aligns the "noisy" vision telemetry from a live game session with the "perfect" deterministic data from a replay file.

```powershell
# Merge a live telemetry CSV with its corresponding replay file
cargo run --bin mesher "path/to/telemetry.csv" "path/to/match.aoe2record"
```

The Meshing process utilizes several key strategies to ensure precision:
*   **Two-Point Linear Calibration**: Automatically detects game speed (e.g., 1.7x, 2.0x) and calculates the exact game-start offset by anchoring to the first resource drop and the completion of Feudal Age research.
*   **First-Frame Ground Truth**: Dynamically extracts starting resources from the first telemetry frame, ensuring compatibility with all civilizations (including bonuses like Chinese or Hindustanis) and game modes.
*   **Causal Ordering**: Events are sorted such that "Replay Actions" (e.g., clicking a button) always appear before the resulting "Telemetry Updates" (e.g., resources dropping) within the same millisecond.

The output is a unified `output/merged_observations.csv` that serves as the foundation for Phase 5: State Snapshotting.


### System Requirements (Live Capture)
*   **Operating System**: Windows 10/11
*   **Display**: Primary monitor must be active (DXGI does not support headless sessions).
*   **Performance**: The capture loop is throttled to ~2 FPS to maintain <1% CPU impact during gameplay.

## Data Acknowledgements

This project utilizes community-standard data and mapping provided by the following projects:

*   **Unit Statistics**: [unitstatistics.com](https://unitstatistics.com/age-of-empires2/)
*   **Object & Tech Tables**: [airef.github.io](https://airef.github.io/tables/objects.html)
*   **Halfon Data**: [halfon.aoe2.se](https://halfon.aoe2.se/) and [SiegeEngineers/halfon](https://github.com/SiegeEngineers/halfon)

