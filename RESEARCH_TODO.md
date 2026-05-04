# Research & Development Tasks

## Phase 1: R&D Proof of Concept (Screen Scraping Validation)
**Goal:** Prove we can reliably extract data from *any* resolution without requiring the user to configure coordinates manually.

- [x] **Data Gathering (The "Test Bench"):**
    - [x] Collect 5-10 screenshots of AoE2:DE from different setups (Saved in `test_bench/`):
        - [x] Standard (16:9) -> `aoe2_16x9.png`
        - [x] Aspect Ratio Variant (16:10) -> `aoe2_16x10.png`
        - [x] 4K (High DPI) -> `aoe2_4k.png`
        - [x] High/Low UI Scale variants -> `aoe2_16x9_max.png`, `aoe2_16x9_min.png`
        - [x] Ultrawide (21:9 & 32:9) -> `aoe2_21x9.png`, `aoe2_32x9.png`
- [x] **UI Map Logic:**
    - [x] Define the `ui_map.json` structure (Anchors + Percentage Offsets).
    - [x] Identify a reliable "Anchor Image" (The rightmost red UI element cluster).
- [x] **Prototype Script (`poc_vision.py`):**
    - [x] Implementation: Python + OpenCV.
    - [x] Input: The "Test Bench" folder of screenshots.
    - [x] Process: Locate anchor -> Calculate scale & **XY Translation** -> Crop regions -> OCR digits.
    - [x] Output: JSON identifying Resource values and Villager counts for each screenshot.
- [x] **Validation:**
    - [x] Verify that the system correctly reads values from different resolutions. (Verified in Rust: Scaling and live capture fully operational).

## Phase 2: Python Validation (Pre-Rust Port)
**Goal:** Validate the full live pipeline in Python before porting to Rust, so we're not porting a moving target.

- [x] **Live Screen Capture:** `poc_capture.py` — confirmed working in Python; now fully implemented in Rust using DXGI for sub-40ms latency.
- [x] **Performance Baseline:** Rust implementation achieves ~30ms per frame (<1% CPU impact), comfortably exceeding the 2 FPS target.

## Phase 3: Rust Port (Production Binary)
**Goal:** Translate the Python PoC into a high-performance Rust application.

- [x] **Core Vision Pipeline:** Port template matching and digit extraction to `image` and `imageproc` crates.
- [x] **Optimization:** Ensure <1% CPU overhead to prevent game micro-stutters.
- [x] **Release Build:** Verify stable execution with `cargo build --release`.

## Phase 4: Telemetry & Replay Sync (Validation)
**Goal:** Prove accuracy by meshing live telemetry CSV with absolute ground truth from replay files.

- [x] **CSV Export:** Rust capture loop now logs high-performance telemetry to CSV.
- [x] **Replay Parser (Extraction)**: Tool implemented to extract tech events and unit queuing with accurate mm:ss.sss timestamps.
    - [x] Support for `data/techs.csv` and `data/units.csv` for ID mapping.
    - [x] Internal state tracking for Building Instance IDs to resolve Building Types for Research and Unqueue events.
    - [x] Implement Building Construction extraction to achieve 100% building type coverage.
    - [x] Implement Resource Cost calculation in extraction output.
- [x] **Sync Tool:** Build a "Mesher" that aligns timestamps and verifies resource drops. (Complete: `mesher.rs` handles sub-second temporal alignment).
- [x] **State Interpolation Logic:** Validate the temporal bridging of "housed" and "queued" overlays during vision flickers.
    - [x] Implementation: `interpolation_engine.py` (Supports housed priority and transition bridging).
    - [x] Verification: `test_interpolation.py` with expanded synthetic scenarios in `test_bench/interpolation/`.


## Future Research (Post-Core-Tool)
- [ ] **Anti-Cheat Deep Dive:** Verify legal safety of background scraping for StarCraft 2 and AoM:R.
- [ ] **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
- [ ] **AoE2 Object ID Sequence:** Investigate how the engine assigns IDs to foundations and units to enable 100% accurate mapping without explicit Interact actions.
- [ ] **Trust Bench:** Research and price-match EV Certificate providers (Sectigo, DigiCert, GlobalSign).
- [ ] **(Low Priority) CaptureAge Pro API:** Investigate only as an optional feature for pro users.