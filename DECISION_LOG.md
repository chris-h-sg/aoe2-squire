# Decision Log & Architecture Evolution

## [DECIDED] Local-First vs. Cloud-Service
- **Option 1 (Discarded):** Cloud-based Replay-as-a-Service.
    - *Reasoning:* High compute costs ($0.70/hr for GPU instances), DRM/Steam licensing hurdles for server-side game instances, and no "headless" mode for the engine.
- **Option 2 (Selected):** Local Desktop Agent.
    - *Reasoning:* Zero server cost (user provides GPU/License), immediate access to local file system, and better privacy/trust potential.

## [DECIDED] Screen Scraping vs. Memory Hooking
- **Option 1 (Discarded):** Memory Hooking (Directly reading RAM).
    - *Reasoning:* High risk of Anti-Cheat bans, extremely fragile (breaks every game patch), requires Admin permissions.
- **Option 2 (Selected):** Passive Screen Scraping (OCR/Template Matching).
    - *Reasoning:* Safer (passive), resilient to internal engine changes, works across different games with the same logic, no Admin rights needed.

## [DECIDED] Primary Data Source Priority
- **Option 1 (Discarded):** CaptureAge Pro API as Primary.
    - *Reasoning:* Restricts the user base to those who pay for/install CaptureAge Pro. Creates an unacceptable barrier to entry for a "mass market" tool.
- **Option 2 (Selected):** Screen Scraping as Primary.
    - *Reasoning:* Works for every user "out of the box." CaptureAge integration will remain as a "High Fidelity" optional plugin for power users, but the core product must function 100% without it.

## [PENDING] Desktop Framework
- **Candidate:** Tauri (Rust).
    - *Pros:* Tiny installer (~6MB), high performance for CV, native access to Windows DXGI, handles "User-land" permissions.
- **Alternative:** Electron.
- **Status:** **Decision Pending.**

## [DECIDED] UI Strategy: Tray-based Agent vs. CLI
- **Decision:** Tray-based Desktop Agent.
- **Reasoning:** Building a CLI-only MVP for a gaming audience is a "Developer Trap." UX friction (copying files, manual commands) kills adoption. A background tray app that "just works" when the game starts is mandatory for trust and retention.

## [PENDING] Feedback Timing
- **Strategy 1:** Live "In-ear" coaching alerts. (High risk of being flagged as a cheat).
- **Strategy 2:** Instant Post-Game Report. (100% safe, focus of the MVP).