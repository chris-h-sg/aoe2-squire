use rts_analyzer::analysis::mesher;
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: cargo run --bin mesher <path/to/csv> <path/to/replay>");
        std::process::exit(1);
    }

    let csv_path = Path::new(&args[1]);
    let replay_path = Path::new(&args[2]);

    let merged = mesher::generate_merged_observations(csv_path, replay_path)?;

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
            record.housing,
        ])?;
    }

    println!("Wrote merged observations to {}", out_path.display());
    Ok(())
}
