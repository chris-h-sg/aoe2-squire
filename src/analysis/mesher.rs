use crate::analysis::smoothing;
use crate::replay::{extract_events, ReplayEvent};
use crate::sync::{Calibration, TimeMapping};
use crate::types::{CsvRow, HousingStatus, Observation};
use csv::ReaderBuilder;
use std::path::Path;

/// Without a replay there is nothing to sync with; telemetry is placed on the in-game
/// clock at DE's Normal speed from the first frame with every resource readable.
const TELEMETRY_ONLY_SPEED: f64 = 1.7;

pub fn generate_merged_observations(
    csv_path: &Path,
    replay_path: Option<&Path>,
    min_spike: i32,
    max_duration: usize,
) -> Result<Vec<Observation>, Box<dyn std::error::Error>> {
    generate_merged_observations_with_calibration(csv_path, replay_path, min_spike, max_duration)
        .map(|(merged, _)| merged)
}

/// Same as `generate_merged_observations`, but also returns the replay/CSV time
/// calibration (None when no replay was given).
pub fn generate_merged_observations_with_calibration(
    csv_path: &Path,
    replay_path: Option<&Path>,
    min_spike: i32,
    max_duration: usize,
) -> Result<(Vec<Observation>, Option<Calibration>), Box<dyn std::error::Error>> {
    let csv_rows = load_telemetry(csv_path, min_spike, max_duration)?;

    if let Some(replay) = replay_path {
        let events = load_player_events(replay)?;
        let calibration = crate::sync::calibrate(&csv_rows, &events)?;
        let merged = merge(csv_rows, events, &calibration.mapping);
        Ok((merged, Some(calibration)))
    } else {
        // Find Game Start in CSV (First frame where all resources are detected)
        let start_ts_rw = csv_rows
            .iter()
            .find(|r| r.food.is_some() && r.wood.is_some() && r.gold.is_some() && r.stone.is_some())
            .map(|r| r.timestamp_ms)
            .ok_or("Could not find any frames with resource data in CSV.")?;
        let mapping = TimeMapping::linear(start_ts_rw as f64, TELEMETRY_ONLY_SPEED);
        Ok((merge(csv_rows, vec![], &mapping), None))
    }
}

/// Reads the telemetry CSV and smooths OCR outliers in the villager count.
pub fn load_telemetry(
    csv_path: &Path,
    min_spike: i32,
    max_duration: usize,
) -> Result<Vec<CsvRow>, Box<dyn std::error::Error>> {
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
            let pop_vils = record.get(11).and_then(|s| s.parse().ok());
            let idle_vils = record.get(12).and_then(|s| s.parse().ok());
            let housing = record.get(13).and_then(HousingStatus::parse);
            csv_rows.push(CsvRow {
                timestamp_ms: ts,
                food,
                wood,
                gold,
                stone,
                pop_curr,
                pop_max,
                pop_vils,
                idle_vils,
                housing,
            });
        }
    }

    if csv_rows.is_empty() {
        return Err("CSV is empty.".into());
    }

    // --- SMOOTHING STEP ---
    // Extract pop_vils into a sequence, smooth it, and put it back
    let mut vils_seq: Vec<Option<u32>> = csv_rows.iter().map(|r| r.pop_vils).collect();
    let smoothed_count = smoothing::smooth_sequence(&mut vils_seq, min_spike, max_duration);
    if smoothed_count > 0 {
        println!(
            "Smoothing: corrected {} OCR outliers in 'pop_vils'.",
            smoothed_count
        );
    }
    for (i, row) in csv_rows.iter_mut().enumerate() {
        row.pop_vils = vils_seq[i];
    }
    Ok(csv_rows)
}

/// The replay events of the recorded (POV) player.
pub fn load_player_events(replay: &Path) -> Result<Vec<ReplayEvent>, Box<dyn std::error::Error>> {
    let data = extract_events(replay)?;
    let rec_player = data.rec_player;
    let mut events = data.events;
    events.retain(|e| e.player_id() as u32 == rec_player);
    Ok(events)
}

/// Places telemetry rows on the in-game clock with `mapping` and merges them with the
/// replay events into one timeline. Rows from before the game started and events after
/// the last captured frame are left out.
pub fn merge(
    csv_rows: Vec<CsvRow>,
    mut events: Vec<ReplayEvent>,
    mapping: &TimeMapping,
) -> Vec<Observation> {
    let last_captured_ig_ms = csv_rows.last().map_or(0.0, |r| {
        mapping.ig_at(r.timestamp_ms as f64).max(0.0).round()
    });
    let original_event_count = events.len();
    events.retain(|e| e.timestamp_ms() as f64 <= last_captured_ig_ms);
    if events.len() < original_event_count {
        println!(
            "Mesher: Ignored {} replay events that occurred after the final captured frame.",
            original_event_count - events.len()
        );
    }

    // Add Rec Events FIRST (so they appear before ScreenGrabs at same ms)
    let mut merged: Vec<Observation> = events.into_iter().map(Observation::Event).collect();
    for row in csv_rows {
        let in_game_ms = mapping.ig_at(row.timestamp_ms as f64);
        if in_game_ms >= 0.0 {
            merged.push(Observation::Screen {
                in_game_ms: in_game_ms.round() as u64,
                reading: row,
            });
        }
    }

    merged.sort_by_key(|o| o.in_game_ms());
    merged
}
