use aoe2_squire::analysis::mesher;
use aoe2_squire::constants::{
    CAPTURE_INTERVAL_MS, SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE,
};
use aoe2_squire::validation;
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if paths.is_empty() || paths.len() > 2 {
        eprintln!(
            "Usage: cargo run --bin sanity_report -- <path/to/csv> [path/to/replay] [--json]"
        );
        std::process::exit(1);
    }

    let (merged, calibration) = mesher::generate_merged_observations_with_calibration(
        Path::new(paths[0]),
        paths.get(1).map(Path::new),
        SMOOTH_VILS_MIN_SPIKE,
        (SMOOTH_VILS_MAX_DURATION_MS / CAPTURE_INTERVAL_MS) as usize,
    )?;
    let report = validation::check(&merged);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        if let Some(c) = calibration {
            println!(
                "Sync: speed {:.4}, Feudal anchor {}",
                c.speed_factor,
                if c.anchor2_rw > 0 {
                    "found"
                } else {
                    "NOT found (fallback speed)"
                }
            );
        }
        print!("{}", report.to_text());
    }
    Ok(())
}
