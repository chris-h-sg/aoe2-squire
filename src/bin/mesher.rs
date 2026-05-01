use csv::ReaderBuilder;
use rts_analyzer::replay::{extract_events, ReplayEvent};
use std::env;
use std::path::Path;

#[derive(Debug, Clone)]
struct CsvRow {
    timestamp_ms: u64,
    food: Option<u32>,
    wood: Option<u32>,
    gold: Option<u32>,
    stone: Option<u32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: cargo run --bin mesher <path/to/csv> <path/to/replay>");
        std::process::exit(1);
    }
    
    let csv_path = &args[1];
    let replay_path = Path::new(&args[2]);

    // 1. Parse CSV
    let mut rdr = ReaderBuilder::new().from_path(csv_path)?;
    let mut csv_rows = Vec::new();
    for result in rdr.records() {
        let record = result?;
        if record.len() >= 9 {
            let ts: u64 = record[0].parse()?;
            let food = record[1].parse().ok();
            let wood = record[3].parse().ok();
            let gold = record[5].parse().ok();
            let stone = record[7].parse().ok();
            csv_rows.push(CsvRow { timestamp_ms: ts, food, wood, gold, stone });
        }
    }

    // Find Game Start in CSV (200F, 200W, 100G, 200S)
    let mut start_ts_rw = 0;
    for row in &csv_rows {
        if row.food == Some(200) && row.wood == Some(200) && row.gold == Some(100) && row.stone == Some(200) {
            start_ts_rw = row.timestamp_ms;
            break;
        }
    }
    if start_ts_rw == 0 {
        eprintln!("Could not find game start (200/200/100/200) in CSV.");
        std::process::exit(1);
    }
    println!("Game start real-world timestamp: {}", start_ts_rw);

    // 2. Parse Replay
    let (rec_owner, _player_names, mut events) = extract_events(replay_path)?;
    println!("Recorded by player ID: {}", rec_owner);
    
    // Filter events to only the recorded player
    events.retain(|e| {
        let p_id = match e {
            ReplayEvent::TechResearch { player_id, .. } => *player_id,
            ReplayEvent::UnitQueued { player_id, .. } => *player_id,
            ReplayEvent::QueueCancellation { player_id, .. } => *player_id,
            ReplayEvent::BuildingConstruction { player_id, .. } => *player_id,
            ReplayEvent::Deletion { player_id, .. } => *player_id,
        };
        p_id as u32 == rec_owner
    });
    
    // Adjust for Hindustanis (46 food villager workaround)
    for ev in events.iter_mut() {
        if let ReplayEvent::UnitQueued { cost, unit_type, .. } = ev {
            if unit_type == "Villager" {
                cost.food = 46; // Temporary workaround
            }
        }
    }

    // 3. Detect Game Speed & Offset (Two-Point Calibration)
    let mut speed_factor = 1.7; // default
    let mut detected = false;
    let mut anchor1_ig = 0;
    let mut anchor1_rw = 0;
    
    // Find starting resources from the FIRST frame (ground truth)
    let first_frame = csv_rows.iter().find(|r| r.timestamp_ms >= start_ts_rw).ok_or("No valid screen grabs found")?;
    let start_food = first_frame.food.unwrap_or(200);
    let start_wood = first_frame.wood.unwrap_or(200);
    let start_gold = first_frame.gold.unwrap_or(100);
    let start_stone = first_frame.stone.unwrap_or(200);
    println!("Starting resources (from screen): Food={}, Wood={}, Gold={}, Stone={}", 
        start_food, start_wood, start_gold, start_stone);

    // Find first drop in CSV
    let mut first_drop_idx = None;
    let mut first_drop_food_delta = 0;
    let mut first_drop_wood_delta = 0;
    for (i, row) in csv_rows.iter().enumerate() {
        if row.timestamp_ms >= start_ts_rw {
            if row.food < Some(start_food) || row.wood < Some(start_wood) {
                first_drop_idx = Some(i);
                first_drop_food_delta = start_food - row.food.unwrap_or(start_food);
                first_drop_wood_delta = start_wood - row.wood.unwrap_or(start_wood);
                anchor1_rw = row.timestamp_ms;
                break;
            }
        }
    }

    if let Some(_) = first_drop_idx {
        // Find matching events in Replay
        let mut cum_food = 0;
        let mut cum_wood = 0;
        for ev in &events {
            let (ig_ms, cost) = match ev {
                ReplayEvent::TechResearch { timestamp_ms, cost, .. } => (timestamp_ms, Some(cost)),
                ReplayEvent::UnitQueued { timestamp_ms, cost, .. } => (timestamp_ms, Some(cost)),
                ReplayEvent::BuildingConstruction { timestamp_ms, cost, .. } => (timestamp_ms, Some(cost)),
                _ => (&0, None),
            };
            if let Some(c) = cost {
                cum_food += c.food;
                cum_wood += c.wood;
                if cum_food >= first_drop_food_delta && cum_wood >= first_drop_wood_delta {
                    anchor1_ig = *ig_ms as u64 + 1;
                    break;
                }
            }
        }
    }

    // Anchor 2: Feudal Age
    let mut anchor2_ig = 0;
    let mut anchor2_rw = 0;
    let mut feudal_ev = None;
    for ev in &events {
        if let ReplayEvent::TechResearch { timestamp_ms, tech_type, .. } = ev {
            if tech_type == "Feudal Age" {
                feudal_ev = Some(*timestamp_ms);
                break;
            }
        }
    }

    if let Some(ig_ms) = feudal_ev {
        // Look for 450+ food drop in CSV near expected time (using default speed to bound search)
        let est_rw = anchor1_rw + ((ig_ms as u64 - anchor1_ig) as f64 / 1.7) as u64;
        let mut prev_food = 0;
        for row in &csv_rows {
            if row.timestamp_ms > est_rw - 10000 && row.timestamp_ms < est_rw + 10000 {
                if let (Some(f), pf) = (row.food, prev_food) {
                    if pf > 450 && f < 100 && pf - f >= 450 {
                        anchor2_rw = row.timestamp_ms;
                        anchor2_ig = ig_ms as u64 + 1;
                        detected = true;
                        break;
                    }
                }
            }
            prev_food = row.food.unwrap_or(0);
        }
    }

    let game_start_rw_f64;
    if detected && anchor2_rw > anchor1_rw {
        speed_factor = (anchor2_ig - anchor1_ig) as f64 / (anchor2_rw - anchor1_rw) as f64;
        game_start_rw_f64 = anchor1_rw as f64 - (anchor1_ig as f64 / speed_factor);
        println!("Detected calibration:");
        println!("  Speed factor: {:.4}", speed_factor);
        println!("  Game start offset (RW): {:.3}", game_start_rw_f64);
        println!("  Anchor 1 (Start): IG {} -> RW {}", anchor1_ig, anchor1_rw);
        println!("  Anchor 2 (Feudal): IG {} -> RW {}", anchor2_ig, anchor2_rw);
    } else {
        println!("Could not dynamically calibrate using Feudal Age, using default 1.7 and start anchor.");
        if anchor1_rw > 0 {
            game_start_rw_f64 = anchor1_rw as f64 - (anchor1_ig as f64 / 1.7);
            println!("  Using start anchor: IG {} -> RW {}", anchor1_ig, anchor1_rw);
        } else {
            game_start_rw_f64 = start_ts_rw as f64;
        }
    }

    // 4. Create merged stream
    // (in_game_ms, type, event_desc, food, wood, gold, stone, rw_timestamp, building_type, building_ids)
    let mut merged: Vec<(u64, String, String, String, String, String, String, String, String, String)> = Vec::new();
    
    // Add Rec Events FIRST (so they appear before ScreenGrabs at same ms)
    for ev in &events {
        let (ig_ms, desc, cost, b_type, b_ids) = match ev {
            ReplayEvent::TechResearch { timestamp_ms, tech_type, cost, building_type, building_id, .. } => 
                (timestamp_ms, format!("Research {}", tech_type), Some(cost), building_type.clone(), building_id.to_string()),
            ReplayEvent::UnitQueued { timestamp_ms, unit_type, cost, building_types, building_ids, .. } => 
                (timestamp_ms, format!("Queue {}", unit_type), Some(cost), building_types.join(","), building_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
            ReplayEvent::QueueCancellation { timestamp_ms, building_types, building_ids, .. } => 
                (timestamp_ms, "Unqueue".to_string(), None, building_types.join(","), building_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
            ReplayEvent::BuildingConstruction { timestamp_ms, building_type, cost, .. } => 
                (timestamp_ms, format!("Build {}", building_type), Some(cost), building_type.clone(), "".to_string()),
            ReplayEvent::Deletion { timestamp_ms, object_name, .. } => 
                (timestamp_ms, format!("Delete {}", object_name), None, "".to_string(), "".to_string()),
        };

        let f = cost.filter(|c| c.food > 0).map(|c| format!("-{}", c.food)).unwrap_or_default();
        let w = cost.filter(|c| c.wood > 0).map(|c| format!("-{}", c.wood)).unwrap_or_default();
        let g = cost.filter(|c| c.gold > 0).map(|c| format!("-{}", c.gold)).unwrap_or_default();
        let s = cost.filter(|c| c.stone > 0).map(|c| format!("-{}", c.stone)).unwrap_or_default();

        merged.push((*ig_ms as u64, "RecEvent".to_string(), desc, f, w, g, s, "".to_string(), b_type, b_ids));
    }

    // Add CSV rows
    for row in &csv_rows {
        if row.timestamp_ms as f64 >= game_start_rw_f64 {
            let ig_ms = ((row.timestamp_ms as f64 - game_start_rw_f64) * speed_factor).round() as u64;
            merged.push((ig_ms, "ScreenGrab".to_string(), "".to_string(), 
                row.food.map(|v| v.to_string()).unwrap_or_default(),
                row.wood.map(|v| v.to_string()).unwrap_or_default(),
                row.gold.map(|v| v.to_string()).unwrap_or_default(),
                row.stone.map(|v| v.to_string()).unwrap_or_default(),
                row.timestamp_ms.to_string(),
                "".to_string(),
                "".to_string(),
            ));
        }
    }

    merged.sort_by_key(|k| k.0);

    // 5. Write to CSV
    let out_dir = Path::new("output");
    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir)?;
    }
    let out_path = out_dir.join("merged_observations.csv");
    let mut wtr = csv::Writer::from_path(&out_path)?;
    wtr.write_record(&["in_game_ms", "observation_type", "event_desc", "food", "wood", "gold", "stone", "rw_timestamp_ms", "building_type", "building_ids"])?;
    for record in merged {
        wtr.write_record(&[
            record.0.to_string(),
            record.1,
            record.2,
            record.3,
            record.4,
            record.5,
            record.6,
            record.7,
            record.8,
            record.9,
        ])?;
    }
    
    println!("Wrote merged observations to {}", out_path.display());
    Ok(())
}
