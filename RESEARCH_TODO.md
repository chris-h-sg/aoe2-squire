# Research & Development Tasks

## Phase 1: R&D Proof of Concept (Screen Scraping Validation)
**Goal:** Prove we can reliably extract data from *any* resolution without requiring the user to configure coordinates manually.

- [ ] **Data Gathering (The "Test Bench"):**
    - [x] Collect 5-10 screenshots of AoE2:DE from different setups (Saved in `test_bench/`):
        - [x] Standard (16:9) -> `aoe2_16x9.png`
        - [x] Aspect Ratio Variant (16:10) -> `aoe2_16x10.png`
        - [x] 4K (High DPI) -> `aoe2_4k.png`
        - [ ] "UI Scale" variants (e.g., 125% scale in-game)
        - [x] Ultrawide (21:9 & 32:9) -> `aoe2_21x9.png`, `aoe2_32x9.png`
- [ ] **UI Map Logic:**
    - [ ] Define the `ui_map.json` structure (Anchors + Percentage Offsets).
    - [ ] Identify a reliable "Anchor Image" (e.g., the current Age icon or Minimap corner) that is consistent across all UI scales.
- [ ] **Prototype Script (`poc_vision.py`):**
    - [x] Implementation: Python + OpenCV.
    - [x] Input: The "Test Bench" folder of screenshots.
    - [ ] Process: Locate anchor -> Calculate offsets -> Crop regions -> OCR digits.
    - [ ] Output: JSON identifying Resource values and Villager counts for each screenshot.
- [ ] **Validation:**
    - [ ] Verify that the script correctly reads values from at least 3 different resolutions without code changes.

## Phase 2: Technical Research
- [ ] **Anti-Cheat Deep Dive:** Verify legal safety of background scraping for StarCraft 2 and AoM:R.
- [ ] **Gather Rate Database:** Compile a master JSON of base gather rates and tech multipliers.
- [ ] **Trust Bench:** Research and price-match EV Certificate providers (Sectigo, DigiCert, GlobalSign).
- [ ] **(Low Priority) CaptureAge Pro API:** Investigate only as an optional feature for pro users.