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
    - [x] Verify that the script correctly reads values from different resolutions without code changes. (In progress: scaling works, shifting pending)

## Phase 2: Python Validation (Pre-Rust Port)
**Goal:** Validate the full live pipeline in Python before porting to Rust, so we're not porting a moving target.

- [ ] **Live Screen Capture:** Write a short script (replacing `poc_video.py`'s file loop with `mss` or `dxcam` grabs) that runs the existing `poc_vision.process_frame()` against the actual running game. Validates anchor detection on real frames and catches any DPI/HDR surprises not present in recordings.
- [ ] **Performance Baseline:** While running live capture, measure per-frame processing time to confirm the pipeline stays within budget at the target sampling rate (2–4 frames/sec).

## Phase 3: Future Research (Post-Core-Tool)
- [ ] **Anti-Cheat Deep Dive:** Verify legal safety of background scraping for StarCraft 2 and AoM:R.
- [ ] **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
- [ ] **Trust Bench:** Research and price-match EV Certificate providers (Sectigo, DigiCert, GlobalSign).
- [ ] **(Low Priority) CaptureAge Pro API:** Investigate only as an optional feature for pro users.