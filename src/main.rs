use aoe2_squire::analysis::run;
use aoe2_squire::constants::{SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE};
use aoe2_squire::{capture, pipeline, report, types};
use std::error::Error;
use std::fs;
use std::path::Path;

fn run_full_analysis(
    csv_path: &Path,
    replay_path: Option<&Path>,
    verbose: bool,
    min_spike: i32,
    max_duration_ms: u64,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== STARTING INTEGRATED ANALYSIS PIPELINE ===");
    println!("CSV: {}", csv_path.display());
    if let Some(replay) = replay_path {
        println!("Replay: {}", replay.display());
    } else {
        println!("Replay: None (Telemetry Only Fallback)");
    }
    println!(
        "Smoothing: min_spike={}, max_duration_ms={}",
        min_spike, max_duration_ms
    );

    let analysis = run::analyze_match(csv_path, replay_path, verbose, min_spike, max_duration_ms)?;
    let metadata = analysis.metadata;
    let merged = analysis.merged;
    let idle_stats = analysis.idle;
    let housing_stats = analysis.housing;
    let floating_stats = analysis.floating;

    // 3. Generate Report
    println!("\nGenerating HTML report...");
    let chart_data = report::extract_chart_data(&merged, &floating_stats);
    let report_html = report::generate_report(
        &metadata,
        &idle_stats,
        &housing_stats,
        &floating_stats,
        &chart_data,
    )?;

    let output_path = Path::new("output").join(report::report_filename(&metadata));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, report_html)?;

    println!("Opening report: {}", output_path.display());
    if let Err(e) = open::that(&output_path) {
        eprintln!("Could not open the report in a browser: {}", e);
    }

    println!("\n=== ANALYSIS COMPLETE ===");
    Ok(())
}

fn main() {
    // Set a panic hook to ensure we wait for user input even on unexpected crashes
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("\n========================================");
        eprintln!("A fatal error occurred (panic):");
        eprintln!("{}", panic_info);
        eprintln!("\nPlease report this error via Discord:");
        eprintln!("https://discord.gg/A9QXpDUHX");
        eprintln!("========================================");
        wait_for_user();
    }));

    // Set a thematic Ctrl-C handler to exit gracefully and prevent ugly console error dumps
    let _ = ctrlc::set_handler(move || {
        aoe2_squire::RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        println!(
            "\n\n♦ Godspeed, my liege! Your Squire stands ready to serve at your next bidding."
        );
        wait_for_user();
        std::process::exit(0);
    });

    if let Err(e) = run_app() {
        eprintln!("\n========================================");
        eprintln!("ERROR: {}", e);
        eprintln!("\nPlease report this error via Discord:");
        eprintln!("https://discord.gg/A9QXpDUHX");
        eprintln!("========================================");
        wait_for_user();
        std::process::exit(1);
    }
}

/// Analyzes a captured match without propagating errors, so the background loop
/// survives a bad match. If the replay-based analysis fails (e.g. the replay format
/// changed after a game patch), falls back to a telemetry-only report.
/// Returns whether a report was generated.
fn analyze_captured_match<F>(replay: Option<&Path>, mut analyze: F) -> bool
where
    F: FnMut(Option<&Path>) -> Result<(), Box<dyn Error>>,
{
    let err = match analyze(replay) {
        Ok(()) => return true,
        Err(e) => e,
    };

    let err = if replay.is_some() {
        eprintln!("\n[Analysis] Replay-based analysis failed: {}", err);
        eprintln!("[Analysis] Falling back to a telemetry-only report.");
        match analyze(None) {
            Ok(()) => return true,
            Err(e) => e,
        }
    } else {
        err
    };

    eprintln!("\n========================================");
    eprintln!(
        "ERROR: The report for this match could not be generated: {}",
        err
    );
    eprintln!("\nPlease report this error via Discord:");
    eprintln!("https://discord.gg/A9QXpDUHX");
    eprintln!("========================================");
    false
}

fn wait_for_user() {
    use std::io::{self, BufRead, Write};
    println!("\nPress Enter to exit...");
    let _ = io::stdout().flush();
    let stdin = io::stdin();
    let mut iterator = stdin.lock().lines();
    let _ = iterator.next();
}

fn run_app() -> Result<(), Box<dyn Error>> {
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
        return run_full_analysis(
            csv_path,
            Some(replay_path),
            verbose,
            min_spike,
            max_duration_ms,
        );
    }

    let ui_map: types::UiMap = {
        let data = include_str!("../ui_map.json");
        serde_json::from_str(data).expect("failed to parse embedded ui_map.json")
    };

    let templates = pipeline::matcher::load_embedded_templates();

    if args.len() > 1 && args[1] == "--parse-replay" {
        if args.len() < 3 {
            eprintln!("Usage: aoe2-squire --parse-replay <path/to/replay.aoe2record>");
            std::process::exit(1);
        }
        let replay_path = Path::new(&args[2]);
        println!("Parsing replay: {}", replay_path.display());
        aoe2_squire::replay::print_events(replay_path)?;
        return Ok(());
    }

    // Re-read frames saved with --record-frames through the capture loop:
    // cargo run -- --replay-frames <dir>  (writes <dir>/replayed/telemetry_*.csv)
    if args.len() > 1 && args[1] == "--replay-frames" {
        if args.len() < 3 {
            eprintln!("Usage: aoe2-squire --replay-frames <recorded frames dir>");
            std::process::exit(1);
        }
        let dir = Path::new(&args[2]);
        let mut source = capture::source::ImageSequenceSource::from_dir(dir)?;
        let opts = capture::CaptureOptions {
            log_dir: dir.join("replayed"),
            ..Default::default()
        };
        match capture::capture_match(&mut source, &ui_map, &templates, &opts)? {
            Some(captured) => println!("Telemetry: {}", captured.telemetry.display()),
            None => eprintln!("No match UI found in {}", dir.display()),
        }
        return Ok(());
    }

    // Saves masked frames of each match next to its telemetry: --record-frames <dir>
    let record_frames = args
        .iter()
        .position(|arg| arg == "--record-frames")
        .and_then(|pos| args.get(pos + 1))
        .map(std::path::PathBuf::from);

    // Any leading flag (--live, --verbose, --record-frames, ...) means live mode.
    if args.len() > 1 && !args[1].starts_with('-') {
        let image_path = &args[1];
        let img = image::open(image_path)
            .map_err(|e| format!("failed to load image {}: {}", image_path, e))?;
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
        println!("╔══════════════════════════════════════╗");
        let version_line = format!("AoE2 Squire v{}", env!("CARGO_PKG_VERSION"));
        println!("║{:^38}║", version_line);
        println!("║{:^38}║", "\"At your service, my liege.\"");
        println!("╚══════════════════════════════════════╝");
        println!();
        println!("To prepare for battle:");
        println!(" ♦ Ensure the game is running on your primary monitor.");
        println!(" ♦ Resolution must be 1366x768 or higher.");
        println!(" ♦ Supported UIs: Default or Anne_HK.");
        println!();
        println!("Simply keep this window open in the background and play as usual.");
        println!("Your Squire will silently observe and generate a report after your next match.");
        println!();
        println!("Feedback & Support: https://discord.gg/A9QXpDUHX");
        println!("════════════════════════════════════════");
        println!();

        // Spawn a background thread to check for the latest release on GitHub
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = aoe2_squire::version::fetch_latest_release_tag("chris-h-sg/aoe2-squire");
            let _ = tx.send(res);
        });

        // Pause for 3 seconds to let the user read the welcome message
        // and allow the background thread to fetch the latest version
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Check if an update is available
        if let Ok(Ok(latest_tag)) = rx.try_recv() {
            let current = env!("CARGO_PKG_VERSION");
            if aoe2_squire::version::is_update_available(current, &latest_tag) {
                println!("♦ A sharper squire ({}) is ready for duty!", latest_tag);
                println!("♦ Recruit them here: https://github.com/chris-h-sg/aoe2-squire/releases/latest");
                println!("════════════════════════════════════════");
                println!();
            }
        }

        loop {
            match capture::run_capture_loop(&ui_map, &templates, record_frames.clone()) {
                Ok(Some((telemetry, replay))) => {
                    println!("\n--- Capture & Discovery Complete ---");
                    let delivered = analyze_captured_match(replay.as_deref(), |replay| {
                        run_full_analysis(&telemetry, replay, verbose, min_spike, max_duration_ms)
                    });
                    println!("\n════════════════════════════════════════");
                    if delivered {
                        println!("♦ The report is delivered, my liege.");
                    }
                    println!("♦ Your Squire is standing by, ready to record your next battle.");
                    println!("════════════════════════════════════════\n");
                }
                Ok(None) => {
                    println!("Capture loop ended without recording a complete session.");
                    println!("\n════════════════════════════════════════");
                    println!("♦ Your Squire remains at his post, waiting for the next battle.");
                    println!("════════════════════════════════════════\n");
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_analysis_success_with_replay_runs_once() {
        let replay = PathBuf::from("match.aoe2record");
        let mut calls = vec![];
        let delivered = analyze_captured_match(Some(&replay), |r| {
            calls.push(r.map(Path::to_path_buf));
            Ok(())
        });
        assert!(delivered);
        assert_eq!(calls, vec![Some(replay)]);
    }

    #[test]
    fn test_replay_analysis_failure_falls_back_to_telemetry_only() {
        let replay = PathBuf::from("match.aoe2record");
        let mut calls = vec![];
        let delivered = analyze_captured_match(Some(&replay), |r| {
            calls.push(r.map(Path::to_path_buf));
            if r.is_some() {
                Err("unsupported replay format".into())
            } else {
                Ok(())
            }
        });
        assert!(delivered);
        assert_eq!(calls, vec![Some(replay), None]);
    }

    #[test]
    fn test_analysis_failure_is_not_propagated() {
        let replay = PathBuf::from("match.aoe2record");
        let mut calls = 0;
        let delivered = analyze_captured_match(Some(&replay), |_| {
            calls += 1;
            Err("boom".into())
        });
        assert!(!delivered);
        assert_eq!(calls, 2);

        let delivered = analyze_captured_match(None, |_| Err("boom".into()));
        assert!(!delivered);
    }
}
