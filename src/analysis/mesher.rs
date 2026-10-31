use crate::analysis::smoothing;
use crate::replay::extract_events;
use crate::sync::Calibration;
use crate::types::{CsvRow, HousingStatus, Observation};
use csv::ReaderBuilder;
use std::path::Path;

pub fn generate_merged_observations(
    csv_path: &Path,
    replay_path: Option<&Path>,
    min_spike: i32,
    max_duration: usize,
) -> Result<Vec<Observation>, Box<dyn std::error::Error>> {
    generate_merged_observations_with_calibration(csv_path, replay_path, min_spike, max_duration)
        .map(|(merged, _)| merged)
}

/// Same as `generate_merged_observations`, but also returns the replay/CSV timing
/// calibration (None when no replay was given).
pub fn generate_merged_observations_with_calibration(
    csv_path: &Path,
    replay_path: Option<&Path>,
    min_spike: i32,
    max_duration: usize,
) -> Result<(Vec<Observation>, Option<Calibration>), Box<dyn std::error::Error>> {
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

    let mut merged: Vec<Observation> = Vec::new();
    let mut calibration_out = None;
    let speed_factor;
    let game_start_rw_f64;

    if let Some(replay) = replay_path {
        // 2. Parse Replay
        let data = extract_events(replay)?;
        let rec_player = data.rec_player;
        let mut events = data.events;

        // Filter events to only the POV player
        events.retain(|e| e.player_id() as u32 == rec_player);

        // 3. Detect Game Speed & Offset (Two-Point Calibration)
        let calibration = crate::sync::calibrate_timing(&csv_rows, &events, start_ts_rw)?;
        speed_factor = calibration.speed_factor;
        game_start_rw_f64 = calibration.game_start_rw_ms;
        calibration_out = Some(calibration);

        // 4. Filter events to ignore anything after the final captured frame
        let last_captured_ig_ms = if let Some(last_row) = csv_rows.last() {
            if last_row.timestamp_ms as f64 >= game_start_rw_f64 {
                ((last_row.timestamp_ms as f64 - game_start_rw_f64) * speed_factor).round() as u64
            } else {
                0
            }
        } else {
            0
        };

        let original_event_count = events.len();
        events.retain(|e| e.timestamp_ms() as u64 <= last_captured_ig_ms);

        if events.len() < original_event_count {
            println!(
                "Mesher: Ignored {} replay events that occurred after the final captured frame.",
                original_event_count - events.len()
            );
        }

        // Add Rec Events FIRST (so they appear before ScreenGrabs at same ms)
        merged.extend(events.into_iter().map(Observation::Event));
    } else {
        speed_factor = 1.7;
        game_start_rw_f64 = start_ts_rw as f64;
    }

    // Add CSV rows
    for row in csv_rows {
        if row.timestamp_ms as f64 >= game_start_rw_f64 {
            let in_game_ms =
                ((row.timestamp_ms as f64 - game_start_rw_f64) * speed_factor).round() as u64;
            merged.push(Observation::Screen {
                in_game_ms,
                reading: row,
            });
        }
    }

    merged.sort_by_key(|o| o.in_game_ms());
    Ok((merged, calibration_out))
}
