# HTML Report Generation - Implementation Plan

This document outlines the strategy for generating the final post-match HTML report in `rts-analyzer`. It covers balancing UI development efficiency with the requirement of producing a single, standalone executable for release.

## Objective
Generate an interactive HTML report summarizing the player's performance (Idle time, Housing penalties, Floating resources) and automatically open it in the user's default browser after the analysis pipeline completes.

## Technology Stack

*   **Templating Engine:** `minijinja` (Jinja2-like syntax)
*   **Auto-Open:** `opener` crate
*   **Visualizations:** Chart.js (v4.5.1) - Bundled directly into the report for offline support.

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

### 2. Asset Bundling (Offline Support)

To ensure the report works without an internet connection, all external assets (like Chart.js) are bundled into the executable using the same pattern.

```rust
pub fn load_chart_js() -> String {
    #[cfg(not(debug_assertions))]
    {
        include_str!("../templates/js/chart.umd.min.js").to_string()
    }

    #[cfg(debug_assertions)]
    {
        fs::read_to_string("templates/js/chart.umd.min.js").expect("...")
    }
}
```

### 3. The Engine

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

## Current Implementation Summary

The report is generated as a standalone HTML file containing all necessary data and styling:

1.  **Consolidated Timeline**: Uses Chart.js to synchronize population datasets, housed status (background shading), age events (markers), and floating resources (top status lanes).
2.  **Categorized Legend**: A custom HTML legend organizes metrics into Economy and Population groups with visibility toggling for each layer.
3.  **Data Serialization**: Rust telemetry is meshed, sampled, and embedded as a JSON payload within the template.
4.  **Styling**: Utilizes a dark-themed CSS layout with high-contrast visual indicators for performance gaps.

## Long-Term Migration Note
If at a later date we desire absolute strict compile-time type safety for our templates, we can migrate from `minijinja` to `askama`. Because both engines utilize the Jinja2 syntax family, the HTML templates will remain exactly the same. The only change required will be replacing the dynamic `minijinja::context!` in Rust with a `#[derive(Template)]` struct.
