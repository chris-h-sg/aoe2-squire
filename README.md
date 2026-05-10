# AoE2 Squire

An AoE2:DE coaching tool that provides performance feedback immediately after your match ends.

It highlights your main areas to improve, such as:
  - **Villager Graph:** Your villager count over time, showing when you stopped producing or lost villagers.
  - **Idle Villagers:** Total gather time lost due to idle villagers.
  - **Housing Efficiency:** Time you spent blocked by your population limit or having more units in queue than available housing pop.
  - **Floating Resources:** Identifies when you stockpile resources for too long. It is smart enough to ignore times when you are intentionally saving up for aging up or building castles.

### Match Report Example

<a href="assets/report-sample.png"><img src="assets/report-sample.png" alt="Sample Post-Game Report showing timeline and efficiency metrics" width="600"></a>

In this example, the chart shows how the player (NotQuiteLegend) is managing their idle villagers well (red line) but could improve their villager production (blue line). They got housed once around 27:00 (red overlay) and floated a lot of gold and stone during Feudal Age (gold and grey bars at the top).

### How It Works

AoE2 Squire runs quietly in the background while you play and creates an interactive Post-Game Report as soon as your match finishes. 

- **Automatic:** No need to upload files or watch replays manually.
- **Zero Setup:** AoE2 Squire is a single standalone program. You don't need to install anything, edit configuration files, or even be connected to the internet.
- **Passive & Safe:** AoE2 Squire only reads your screen visually to track resources. It never hooks into game memory or modifies files, ensuring there is zero risk of anti-cheat bans.

## Getting Started (For Players)

### System Requirements
*   **Operating System**: Windows 10/11
*   **Display**: Game is running on the primary monitor, 1920x1080 or larger.
*   **Game:** Age of Empires II: Definitive Edition.

### Installation
Simply download `aoe2-squire.exe` and place it anywhere on your computer. Everything the tool needs is included directly inside the file.

<details>
<summary><b>Getting a Windows SmartScreen warning?</b></summary>

After downloading AoE2 Squire, Windows SmartScreen might stop it from running because it's not signed by Microsoft.

1. When you see the warning, click on **More info**.

   <img src="assets/smartscreen-1.png" alt="SmartScreen warning showing More info highlighted" width="400">

2. Verify that the Publisher is listed as **Unknown Publisher**. 
   * **Important:** If it shows anything else, do not run the file, as it could be malware!
   * If it shows "Unknown Publisher", it simply means the file isn't signed by Microsoft. Click **Run anyway** to start AoE2 Squire.

   <img src="assets/smartscreen-2.png" alt="SmartScreen warning showing Unknown Publisher and Run anyway button" width="400">
</details>

### How to Use
1. **Start the Tool:** Run `aoe2-squire.exe` before or during your game. It will run quietly in the background with minimal CPU impact, waiting for the game to appear.
2. **Play your Match:** AoE2 Squire automatically detects when the game starts and records your gameplay data.
3. **View the Report:** When the match ends, the tool finds your replay file, matches it with your screen data, and creates an `output/` folder with the HTML report. The report will open automatically in your web browser.
   * *Note: If the tool cannot find your replay file, a window will pop up asking you to select the file yourself.*

### Known Limitations
The vision pipeline currently has a few limitations:
*   **Pausing the Game:** Pausing currently breaks the sync between the visual clock and the replay timeline.
*   **Alt-Tabbing:** If you alt-tab or minimize the game during a match, the visual data will be interrupted.
*   **UI Mods:** Mods that change the position, color, or font of the top resource bar or population counts will break the recorder.
*   **Replay POV:** Analysis is based on the player from whose point of view the replay was recorded.

## Developer Guide (For Contributors)

AoE2 Squire uses a **"Hybrid Data Strategy"**:
1. **Screen Scraping:** Records the "Current State" (resources, idle counts) while the game is running.
2. **Replay Parsing:** Reads the game events (timings for units and technology) from the replay file after the match ends.
3. **Data Meshing:** Combines both timelines to find "Efficiency Gaps" (for example, starting a technology research but failing to spend resources).

For more details, see these documents:
*   `DECISION_LOG.md`: Overall design and architectural choices.
*   `TECHNICAL_SPEC.md`: Project requirements and technical details.
*   `LOW_ELO_ISSUES.md`: Common player mistakes we're trying to help with (many not yet implemented).

### Building from Source
The main code is written in Rust.
```powershell
# Generate the final single-file binary
cargo build --release
```
*The output will be saved at `target/release/aoe2-squire.exe`.*

### Development Workflows

**1. Running the Full Orchestrator:**
```powershell
# Run the automated background orchestrator
cargo run --release

# Run with detailed logs and tables
cargo run --release -- --verbose
```

**2. Manual Analysis Pipeline:**
If you already have a data CSV and a replay file from a past game, you can run the analysis directly:
```powershell
cargo run -- --analyze "path/to/telemetry.csv" "path/to/match.aoe2record"
```

**3. Individual Analyzers (CLI):**
To test specific metrics, you can run analyzers on the `output/merged_observations.csv` file.
```powershell
# Create the merged data file
cargo run --bin mesher "path/to/telemetry.csv" "path/to/match.aoe2record"

# Run specific tests
cargo run --bin idle_analyzer -- --verbose
cargo run --bin housing_analyzer
cargo run --bin floating_analyzer
```

**4. Replay Event Extraction:**
Extract raw events from a replay file:
```powershell
cargo run -- --parse-replay "path/to/match.aoe2record"
```

## Data Acknowledgements
This project uses community data from:
*   **Unit Statistics**: [unitstatistics.com](https://unitstatistics.com/age-of-empires2/)
*   **Object & Tech Tables**: [airef.github.io](https://airef.github.io/tables/objects.html)
*   **Halfon Data**: [halfon.aoe2.se](https://halfon.aoe2.se/) and [SiegeEngineers/halfon](https://github.com/SiegeEngineers/halfon)