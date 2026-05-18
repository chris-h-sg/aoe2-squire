use aoe2_squire::analysis::{floating, housing, idle, mesher};
use aoe2_squire::constants::{
    CAPTURE_INTERVAL_MS, SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE,
};
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

    let metadata = if let Some(replay) = replay_path {
        let replay_data = aoe2_squire::replay::extract_events(replay)?;
        replay_data.metadata
    } else {
        let start_time = if let Some(first_row) = merged.first() {
            if let Ok(ts_rw) = first_row.rw_timestamp_ms.parse::<i64>() {
                use chrono::TimeZone;
                if let Some(datetime) = chrono::Local.timestamp_millis_opt(ts_rw).single() {
                    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                } else {
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
                }
            } else {
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
            }
        } else {
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        };

        let duration_sec = if let (Some(first), Some(last)) = (merged.first(), merged.last()) {
            (last.in_game_ms - first.in_game_ms) as f64 / 1000.0
        } else {
            0.0
        };

        let minutes = (duration_sec / 60.0).floor() as u32;
        let remaining_seconds = duration_sec % 60.0;
        let duration_formatted = format!("{:02}:{:06.3}", minutes, remaining_seconds);

        aoe2_squire::replay::MatchMetadata {
            start_time,
            duration_formatted,
            duration_sec,
            players: vec![aoe2_squire::replay::PlayerInfo {
                name: "Active Player".to_string(),
                civ: "Unknown Civ".to_string(),
            }],
            rec_player_name: "Active Player".to_string(),
            rec_player_civ: "Unknown Civ".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            has_replay: false,
        }
    };

    // 2. Run Analyzers
    let idle_stats = idle::analyze_idle(merged.iter().cloned(), verbose)?;
    let housing_stats = housing::analyze_housing(merged.iter().cloned(), verbose)?;
    let floating_stats = floating::analyze_floating(merged.clone(), verbose)?;

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

    // Construct filename: yyyymmdd-hhmmss-<player name>-<player civilization>.html
    let date_part = metadata
        .start_time
        .replace(['-', ':'], "")
        .replace(' ', "-");
    let safe_player = metadata.rec_player_name.replace(' ', "_");
    let safe_civ = metadata.rec_player_civ.replace(' ', "_");
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

    if args.len() > 1 && args[1] != "--live" {
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
            match capture::run_capture_loop(&ui_map, &templates) {
                Ok(Some((telemetry, replay))) => {
                    println!("\n--- Capture & Discovery Complete ---");
                    run_full_analysis(
                        &telemetry,
                        replay.as_deref(),
                        verbose,
                        min_spike,
                        max_duration_ms,
                    )?;
                    println!("\n════════════════════════════════════════");
                    println!("♦ The report is delivered, my liege.");
                    println!("♦ Your Squire is standing by, ready to record your next battle.");
                    println!("════════════════════════════════════════\n");
                }
                Ok(None) => {
                    println!("Capture loop ended without recording a complete session.");
                    println!("\n════════════════════════════════════════");
                    println!("♦ Your Squire remains at his post, waiting for the next battle.");
                    println!("════════════════════════════════════════\n");
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    Ok(())
}
