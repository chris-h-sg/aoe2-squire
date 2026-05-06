# HTML Report Generation - Implementation Plan

This document outlines the strategy for generating the final post-match HTML report in `rts-analyzer`. It covers how we will balance rapid UI development with the requirement of producing a single, standalone executable for release.

## Objective
Generate a beautifully styled, interactive HTML report summarizing the player's performance (Idle time, Housing penalties, Floating resources) and automatically open it in the user's default browser after the analysis pipeline completes.

## Technology Stack

*   **Templating Engine:** `minijinja` (Jinja2-like syntax)
*   **Auto-Open:** `open` or `webbrowser` crate
*   **Visualizations:** Pure HTML/CSS for MVP, migrating to a lightweight JS library (like `uPlot` or `Chart.js`) for the final graph.

## The "Dual-Environment" Strategy

To achieve zero-compile-time UI iteration while maintaining the single `.exe` mandate for production, we will use conditional compilation macros (`#[cfg]`) to change how the template is loaded based on the build type.

### 1. The Rust Loader

```rust
// In src/report.rs (or similar)

pub fn load_template() -> String {
    // 🔴 RELEASE MODE: Embed the HTML file directly into the binary at compile time
    #[cfg(not(debug_assertions))]
    {
        include_str!("../templates/report.html").to_string()
    }

    // 🟢 DEBUG/DEV MODE: Read the file from the disk at runtime
    #[cfg(debug_assertions)]
    {
        std::fs::read_to_string("templates/report.html")
            .expect("Failed to read templates/report.html. Ensure you are running from the project root.")
    }
}
```

### 2. The Engine

Using `minijinja`, we render the template using the dynamically loaded string:

```rust
use minijinja::{Environment, context};

pub fn generate_report(idle_stats: &IdleReport, housing_stats: &HousingReport, floating_stats: &FloatingReport) -> Result<String, minijinja::Error> {
    let template_str = load_template();
    
    let mut env = Environment::new();
    env.add_template("report", &template_str)?;
    let tmpl = env.get_template("report")?;
    
    tmpl.render(context! {
        idle => idle_stats,
        housing => housing_stats,
        floating => floating_stats,
    })
}
```

## Implementation Phases

### Phase 1: Aggregation & Basic Output (MVP)
1.  **Refactor Analyzers:** Update `analyze_idle`, `analyze_housing`, and `analyze_floating` to return data structures containing their summaries rather than just printing to `stdout`.
2.  **Add Dependencies:** Add `minijinja` and `serde` (for context serialization). Add `open` for launching the browser.
3.  **Create Template Base:** Create `templates/report.html` with basic HTML5 boilerplate and standard CSS styling (e.g., using a dark mode theme suitable for gaming apps).
4.  **Render Tables/Text:** Output the aggregate numbers (e.g., "Total Idle Time: 45s", "Seconds Housed in Feudal: 12s").
5.  **Auto-Launch:** Write the rendered string to `output/report.html` and use `open::that("output/report.html")`.

### Phase 2: The Timeline Graph (CSS Implementation)
For the MVP timeline, we will use a pure CSS Grid/Flexbox approach to plot spans of time.

1.  **Data Preparation:** Convert all events (idle spans, housing blocks) into percentages based on the total game length.
    *   *Example:* If the game is 30 minutes long, an idle block from 15:00 to 16:30 is a `div` that starts at `left: 50%` and has a `width: 5%`.
2.  **Template Injection:** Pass these calculated percentages directly into the template context.
3.  **CSS Rendering:** Render them as stacked, colored blocks on a timeline container.

### Phase 3: Interactive JS Charts (Future Polish)
If the CSS timeline proves too limited for complex overlaps or requires interactive tooltips, we will upgrade to an embedded JS library.

1.  **Inline Library:** Download the minified `.js` file for the chosen library (e.g., `uPlot.min.js`) and place it in the `templates/` folder.
2.  **Inject Script:** Embed the library into the HTML report via `include_str!` or dynamic loading, identically to how the template itself is handled.
3.  **JSON Payload:** Serialize the Rust event data into a raw JSON string and embed it into a `<script>` tag inside the template, initializing the chart.

## Long-Term Migration Note
If at a later date we desire absolute strict compile-time type safety for our templates, we can migrate from `minijinja` to `askama`. Because both engines utilize the Jinja2 syntax family, the HTML templates will remain exactly the same. The only change required will be replacing the dynamic `minijinja::context!` in Rust with a `#[derive(Template)]` struct.
