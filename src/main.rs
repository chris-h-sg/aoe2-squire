use rts_analyzer::analysis::{floating, housing, idle, mesher};
use rts_analyzer::constants::{
    CAPTURE_INTERVAL_MS, SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE,
};
use rts_analyzer::{capture, pipeline, report, types};
use std::error::Error;
use std::fs;
use std::path::Path;

fn run_full_analysis(
    csv_path: &Path,
    replay_path: &Path,
    verbose: bool,
    min_spike: i32,
    max_duration_ms: u64,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== STARTING INTEGRATED ANALYSIS PIPELINE ===");
    println!("CSV: {}", csv_path.display());
    println!("Replay: {}", replay_path.display());
    println!(
        "Smoothing: min_spike={}, max_duration_ms={}",
        min_spike, max_duration_ms
    );

    // Convert duration from ms to frames
    let max_duration_frames = (max_duration_ms / CAPTURE_INTERVAL_MS) as usize;

    // 1. Mesh (includes smoothing)
    let merged = mesher::generate_merged_observations(
        csv_path,
        replay_path,
        min_spike,
        max_duration_frames,
    )?;
    println!("Successfully meshed {} rows.", merged.len());

    let replay_data = rts_analyzer::replay::extract_events(replay_path)?;

    // 2. Run Analyzers
    let idle_stats = idle::analyze_idle(merged.iter().cloned(), verbose)?;
    let housing_stats = housing::analyze_housing(merged.iter().cloned(), verbose)?;
    let floating_stats = floating::analyze_floating(merged.clone(), verbose)?;

    // 3. Generate Report
    println!("\nGenerating HTML report...");
    let chart_data = report::extract_chart_data(&merged, &floating_stats);
    let report_html = report::generate_report(
        &replay_data.metadata,
        &idle_stats,
        &housing_stats,
        &floating_stats,
        &chart_data,
    )?;

    // Construct filename: yyyymmdd-hhmmss-<player name>-<player civilization>.html
    let date_part = replay_data
        .metadata
        .start_time
        .replace('-', "")
        .replace(':', "")
        .replace(' ', "-");
    let safe_player = replay_data.metadata.rec_player_name.replace(' ', "_");
    let safe_civ = replay_data.metadata.rec_player_civ.replace(' ', "_");
    let filename = format!("{}-{}-{}.html", date_part, safe_player, safe_civ);

    let output_path = Path::new("output").join(filename);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, report_html)?;

    println!("Opening report: {}", output_path.display());
    open::that(&output_path)?;

    println!("\n=== ANALYSIS COMPLETE ===");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let verbose = args
        .iter()
        .any(|arg| arg == "--verbose" || arg == "-v" || arg == "--v");

    // Extract smoothing parameters if provided, otherwise use constants
    let mut min_spike = SMOOTH_VILS_MIN_SPIKE;
    let mut max_duration_ms = SMOOTH_VILS_MAX_DURATION_MS;

    if let Some(pos) = args.iter().position(|arg| arg == "--min-spike") {
        if let Some(val) = args.get(pos + 1) {
            if let Ok(v) = val.parse::<i32>() {
                min_spike = v;
            }
        }
    }
    if let Some(pos) = args.iter().position(|arg| arg == "--max-duration") {
        if let Some(val) = args.get(pos + 1) {
            if let Ok(v) = val.parse::<u64>() {
                max_duration_ms = v;
            }
        }
    }

    // Support manual analysis: cargo run -- --analyze <csv> <rec> [--verbose] [--min-spike <n>] [--max-duration <ms>]
    if args.len() >= 4 && args[1] == "--analyze" {
        let csv_path = Path::new(&args[2]);
        let replay_path = Path::new(&args[3]);
        return run_full_analysis(csv_path, replay_path, verbose, min_spike, max_duration_ms);
    }

    let ui_map: types::UiMap = {
        let data = include_str!("../ui_map.json");
        serde_json::from_str(data).expect("failed to parse embedded ui_map.json")
    };

    let templates = pipeline::matcher::load_embedded_templates();

    if args.len() > 1 && args[1] == "--parse-replay" {
        if args.len() < 3 {
            eprintln!("Usage: rts-analyzer --parse-replay <path/to/replay.aoe2record>");
            std::process::exit(1);
        }
        let replay_path = Path::new(&args[2]);
        println!("Parsing replay: {}", replay_path.display());
        rts_analyzer::replay::print_events(replay_path)?;
        return Ok(());
    }

    if args.len() > 1 && args[1] != "--live" {
        let image_path = &args[1];
        let img = image::open(image_path)
            .unwrap_or_else(|_| panic!("failed to load image: {}", image_path));
        println!("Image: {}x{}", img.width(), img.height());

        match pipeline::process_frame(&img, &ui_map, &templates) {
            Some(results) => {
                let mut keys: Vec<_> = results.keys().collect();
                keys.sort();
                for cat in keys {
                    let mut sub: Vec<_> = results[cat].iter().collect();
                    sub.sort_by_key(|(k, _)| *k);
                    for (key, val) in sub {
                        println!("  {}_{}: {}", cat, key, val);
                    }
                }
            }
            None => eprintln!(
                "[no anchor] Red UI reference not found — game not visible or UI changed."
            ),
        }
    } else {
        println!("Starting live screen capture...");
        match capture::run_capture_loop(&ui_map, &templates) {
            Ok(Some((telemetry, replay))) => {
                println!("\n--- Capture & Discovery Complete ---");
                run_full_analysis(&telemetry, &replay, verbose, min_spike, max_duration_ms)?;
            }
            Ok(None) => println!("Capture loop ended without recording a complete session."),
            Err(e) => eprintln!("Capture loop failed: {:?}", e),
        }
    }

    Ok(())
}
