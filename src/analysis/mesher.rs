use crate::replay::{extract_events, ReplayEvent};
use crate::types::{CsvRow, MergedRow};
use csv::ReaderBuilder;
use std::path::Path;

pub fn generate_merged_observations(
    csv_path: &Path,
    replay_path: &Path,
) -> Result<Vec<MergedRow>, Box<dyn std::error::Error>> {
    // 1. Parse CSV
    let mut rdr = ReaderBuilder::new().from_path(csv_path)?;
    let mut csv_rows = Vec::new();
    for result in rdr.records() {
        let record = result?;
        if record.len() >= 13 {
            let ts: u64 = record[0].parse()?;
            let food = record[1].parse().ok();
            let wood = record[3].parse().ok();
            let gold = record[5].parse().ok();
            let stone = record[7].parse().ok();
            let pop_curr = record[9].parse().ok();
            let pop_max = record[10].parse().ok();
            let idle_vils = record.get(12).and_then(|s| s.parse().ok());
            let housing = record.get(13).map(|s| s.to_string());
            csv_rows.push(CsvRow {
                timestamp_ms: ts,
                food,
                wood,
                gold,
                stone,
                pop_curr,
                pop_max,
                idle_vils,
                housing,
            });
        }
    }

    // Find Game Start in CSV (First frame where all resources are detected)
    let mut start_ts_rw = 0;
    for row in &csv_rows {
        if row.food.is_some() && row.wood.is_some() && row.gold.is_some() && row.stone.is_some() {
            start_ts_rw = row.timestamp_ms;
            break;
        }
    }
    if start_ts_rw == 0 {
        return Err("Could not find any frames with resource data in CSV.".into());
    }

    // 2. Parse Replay
    let data = extract_events(replay_path)?;
    let rec_owner = data.rec_owner;
    let mut events = data.events;

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

    // 3. Detect Game Speed & Offset (Two-Point Calibration)
    let calibration = crate::sync::calibrate_timing(&csv_rows, &events, start_ts_rw)?;
    let speed_factor = calibration.speed_factor;
    let game_start_rw_f64 = calibration.game_start_rw_ms;

    // 4. Create merged stream
    let mut merged: Vec<MergedRow> = Vec::new();

    // Add Rec Events FIRST (so they appear before ScreenGrabs at same ms)
    for ev in &events {
        let (ig_ms, desc, cost, b_type, b_ids) = match ev {
            ReplayEvent::TechResearch {
                timestamp_ms,
                tech_type,
                cost,
                building_type,
                building_id,
                ..
            } => (
                timestamp_ms,
                format!("Research {}", tech_type),
                Some(cost),
                building_type.clone(),
                building_id.to_string(),
            ),
            ReplayEvent::UnitQueued {
                timestamp_ms,
                unit_type,
                cost,
                building_types,
                building_ids,
                ..
            } => (
                timestamp_ms,
                format!("Queue {}", unit_type),
                Some(cost),
                building_types.join(","),
                building_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ReplayEvent::QueueCancellation {
                timestamp_ms,
                building_types,
                building_ids,
                ..
            } => (
                timestamp_ms,
                "Unqueue".to_string(),
                None,
                building_types.join(","),
                building_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ReplayEvent::BuildingConstruction {
                timestamp_ms,
                building_type,
                cost,
                ..
            } => (
                timestamp_ms,
                format!("Build {}", building_type),
                Some(cost),
                building_type.clone(),
                "".to_string(),
            ),
            ReplayEvent::Deletion {
                timestamp_ms,
                object_name,
                ..
            } => (
                timestamp_ms,
                format!("Delete {}", object_name),
                None,
                "".to_string(),
                "".to_string(),
            ),
        };

        let f = cost
            .filter(|c| c.food > 0)
            .map(|c| format!("-{}", c.food))
            .unwrap_or_default();
        let w = cost
            .filter(|c| c.wood > 0)
            .map(|c| format!("-{}", c.wood))
            .unwrap_or_default();
        let g = cost
            .filter(|c| c.gold > 0)
            .map(|c| format!("-{}", c.gold))
            .unwrap_or_default();
        let s = cost
            .filter(|c| c.stone > 0)
            .map(|c| format!("-{}", c.stone))
            .unwrap_or_default();

        merged.push(MergedRow {
            in_game_ms: *ig_ms as u64,
            observation_type: "RecEvent".to_string(),
            event_desc: desc,
            food: f,
            wood: w,
            gold: g,
            stone: s,
            rw_timestamp_ms: "".to_string(),
            building_type: b_type,
            building_ids: b_ids,
            idle_vils: "".to_string(),
            pop_curr: "".to_string(),
            pop_max: "".to_string(),
            housing: "".to_string(),
        });
    }

    // Add CSV rows
    for row in &csv_rows {
        if row.timestamp_ms as f64 >= game_start_rw_f64 {
            let ig_ms =
                ((row.timestamp_ms as f64 - game_start_rw_f64) * speed_factor).round() as u64;
            merged.push(MergedRow {
                in_game_ms: ig_ms,
                observation_type: "ScreenGrab".to_string(),
                event_desc: "".to_string(),
                food: row.food.map(|v| v.to_string()).unwrap_or_default(),
                wood: row.wood.map(|v| v.to_string()).unwrap_or_default(),
                gold: row.gold.map(|v| v.to_string()).unwrap_or_default(),
                stone: row.stone.map(|v| v.to_string()).unwrap_or_default(),
                rw_timestamp_ms: row.timestamp_ms.to_string(),
                building_type: "".to_string(),
                building_ids: "".to_string(),
                idle_vils: row.idle_vils.map(|v| v.to_string()).unwrap_or_default(),
                pop_curr: row.pop_curr.map(|v| v.to_string()).unwrap_or_default(),
                pop_max: row.pop_max.map(|v| v.to_string()).unwrap_or_default(),
                housing: row.housing.clone().unwrap_or_default(),
            });
        }
    }

    merged.sort_by_key(|k| k.in_game_ms);
    Ok(merged)
}
