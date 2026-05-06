use rts_analyzer::analysis::{floating, housing, idle, mesher};
use rts_analyzer::{capture, pipeline, report, types};
use std::error::Error;
use std::fs;
use std::path::Path;

fn run_full_analysis(
    csv_path: &Path,
    replay_path: &Path,
    verbose: bool,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== STARTING INTEGRATED ANALYSIS PIPELINE ===");
    println!("CSV: {}", csv_path.display());
    println!("Replay: {}", replay_path.display());

    // 1. Mesh
    let merged = mesher::generate_merged_observations(csv_path, replay_path)?;
    println!("Successfully meshed {} rows.", merged.len());

    let replay_data = rts_analyzer::replay::extract_events(replay_path)?;

    // 2. Run Analyzers
    let idle_stats = idle::analyze_idle(merged.iter().cloned(), verbose)?;
    let housing_stats = housing::analyze_housing(merged.iter().cloned(), verbose)?;
    let floating_stats = floating::analyze_floating(merged, verbose)?;

    // 3. Generate Report
    println!("\nGenerating HTML report...");
    let report_html = report::generate_report(
        &replay_data.metadata,
        &idle_stats,
        &housing_stats,
        &floating_stats,
    )?;
    let output_path = Path::new("output/report.html");
    fs::write(output_path, report_html)?;

    println!("Opening report in default browser...");
    open::that(output_path)?;

    println!("\n=== ANALYSIS COMPLETE ===");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let verbose = args
        .iter()
        .any(|arg| arg == "--verbose" || arg == "-v" || arg == "--v");

    // Support manual analysis: cargo run -- --analyze <csv> <rec> [--verbose]
    if args.len() >= 4 && args[1] == "--analyze" {
        let csv_path = Path::new(&args[2]);
        let replay_path = Path::new(&args[3]);
        return run_full_analysis(csv_path, replay_path, verbose);
    }

    let ui_map: types::UiMap = {
        let data = std::fs::read_to_string("ui_map.json").expect("ui_map.json not found");
        serde_json::from_str(&data).expect("failed to parse ui_map.json")
    };

    let templates_dir = Path::new("research/templates/enormous_numbers");
    let templates = pipeline::matcher::load_templates(templates_dir);

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
                run_full_analysis(&telemetry, &replay, verbose)?;
            }
            Ok(None) => println!("Capture loop ended without recording a complete session."),
            Err(e) => eprintln!("Capture loop failed: {:?}", e),
        }
    }

    Ok(())
}
