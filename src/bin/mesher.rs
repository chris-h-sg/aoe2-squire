use aoe2_squire::analysis::mesher;
use aoe2_squire::constants::{
    CAPTURE_INTERVAL_MS, SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE,
};
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: cargo run --bin mesher <path/to/csv> <path/to/replay> [min_spike] [max_duration_ms]");
        std::process::exit(1);
    }

    let csv_path = Path::new(&args[1]);
    let replay_path = Path::new(&args[2]);
    let min_spike = args
        .get(3)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(SMOOTH_VILS_MIN_SPIKE);
    let max_duration_ms = args
        .get(4)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(SMOOTH_VILS_MAX_DURATION_MS);

    // Convert duration from ms to frames
    let max_duration_frames = (max_duration_ms / CAPTURE_INTERVAL_MS) as usize;

    let merged = mesher::generate_merged_observations(
        csv_path,
        Some(replay_path),
        min_spike,
        max_duration_frames,
    )?;

    // 5. Write to CSV
    let out_dir = Path::new("output");
    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir)?;
    }
    let out_path = out_dir.join("merged_observations.csv");
    let mut wtr = csv::Writer::from_path(&out_path)?;
    wtr.write_record([
        "in_game_ms",
        "observation_type",
        "event_desc",
        "food",
        "wood",
        "gold",
        "stone",
        "rw_timestamp_ms",
        "building_type",
        "building_ids",
        "idle_vils",
        "pop_curr",
        "pop_max",
        "pop_vils",
        "housing",
    ])?;
    for record in merged {
        wtr.write_record([
            record.in_game_ms.to_string(),
            record.observation_type,
            record.event_desc,
            record.food,
            record.wood,
            record.gold,
            record.stone,
            record.rw_timestamp_ms,
            record.building_type,
            record.building_ids,
            record.idle_vils,
            record.pop_curr,
            record.pop_max,
            record.pop_vils,
            record.housing,
        ])?;
    }

    println!("Wrote merged observations to {}", out_path.display());
    Ok(())
}
