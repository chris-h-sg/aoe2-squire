# Project: RTS Analyzer

## Vision
A cross-game RTS coaching tool that provides **instant, narrative-driven feedback** immediately after a match ends. Unlike current tools that require manual uploads or watching replays, RTS Analyzer uses a local agent to provide a "Post-Game Report" the second the match is over.

## The Core Concept
The "Hybrid Data Strategy":
- **Screen Scraping:** Captures the "Current State" (resources, idle counts, villager allocation) during the game without touching memory.
- **Replay Parsing:** Captures the "Deterministic Log" (tech timings, unit production) from the local `.aoe2record` or similar file as soon as the game finishes.
- **Analysis:** Combines both to identify "Efficiency Gaps" (e.g., "You had the tech for faster gathering but your resource intake didn't rise, implying poor lumber camp placement").

## Project Status
- **Phase:** Implementation / MVP.
- **Primary Goal:** Establish the Rust-based DXGI screen-scraping pipeline. The OCR pipeline (segmentation + matching) has been successfully ported from the Python proof-of-concept and validated against test fixtures.

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

By default, the application runs in **Live Capture Mode**. It uses Windows DXGI (Desktop Duplication) to capture your primary monitor every 500ms and stream real-time telemetry to the console:

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

### System Requirements (Live Capture)
*   **Operating System**: Windows 10/11
*   **Display**: Primary monitor must be active (DXGI does not support headless sessions).
*   **Performance**: The capture loop is throttled to ~2 FPS to maintain <1% CPU impact during gameplay.