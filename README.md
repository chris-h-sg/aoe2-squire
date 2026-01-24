# Project: RTS Analyzer

## Vision
A cross-game RTS coaching tool that provides **instant, narrative-driven feedback** immediately after a match ends. Unlike current tools that require manual uploads or watching replays, RTS Analyzer uses a local agent to provide a "Post-Game Report" the second the match is over.

## The Core Concept
The "Hybrid Data Strategy":
- **Screen Scraping:** Captures the "Current State" (resources, idle counts, villager allocation) during the game without touching memory.
- **Replay Parsing:** Captures the "Deterministic Log" (tech timings, unit production) from the local `.aoe2record` or similar file as soon as the game finishes.
- **Analysis:** Combines both to identify "Efficiency Gaps" (e.g., "You had the tech for faster gathering but your resource intake didn't rise, implying poor lumber camp placement").

## Project Status
- **Phase:** Pre-MVP / Architectural Design.
- **Primary Goal:** Validate screen-scraping reliability across resolutions and build a cross-game "UI Map" system.