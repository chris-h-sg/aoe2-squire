use crate::analysis::floating::{self, FloatingReport};
use crate::analysis::housing::{self, HousingReport};
use crate::analysis::idle::{self, IdleReport};
use crate::analysis::mesher;
use crate::constants::CAPTURE_INTERVAL_MS;
use crate::replay::{MatchMetadata, PlayerInfo};
use crate::report::{self, SquireGrades};
use crate::sync::Calibration;
use crate::types::Observation;
use std::error::Error;
use std::path::Path;

/// Everything the HTML report is built from, for one captured match.
pub struct MatchAnalysis {
    /// The merged in-game timeline: screen readings and replay events.
    pub merged: Vec<Observation>,
    /// None when no replay was given (telemetry-only analysis).
    pub calibration: Option<Calibration>,
    pub metadata: MatchMetadata,
    pub idle: IdleReport,
    pub housing: HousingReport,
    pub floating: FloatingReport,
    pub grades: SquireGrades,
}

/// Runs the full analysis path: mesh (incl. smoothing and sync calibration) → idle,
/// housing and floating analyzers → grades.
pub fn analyze_match(
    csv_path: &Path,
    replay_path: Option<&Path>,
    verbose: bool,
    min_spike: i32,
    max_duration_ms: u64,
) -> Result<MatchAnalysis, Box<dyn Error>> {
    // Convert duration from ms to frames
    let max_duration_frames = (max_duration_ms / CAPTURE_INTERVAL_MS) as usize;

    // 1. Mesh (includes smoothing)
    let (merged, calibration) = mesher::generate_merged_observations_with_calibration(
        csv_path,
        replay_path,
        min_spike,
        max_duration_frames,
    )?;
    println!("Successfully meshed {} rows.", merged.len());
    if let Some(c) = &calibration {
        println!("Sync: {}", c.summary());
    }

    let metadata = if let Some(replay) = replay_path {
        let replay_data = crate::replay::extract_events(replay)?;
        replay_data.metadata
    } else {
        telemetry_only_metadata(&merged)
    };

    // 2. Run Analyzers
    let idle_stats = idle::analyze_idle(&merged, verbose)?;
    let housing_stats = housing::analyze_housing(&merged, verbose)?;
    let floating_stats = floating::analyze_floating(&merged, verbose)?;

    let grades = report::calculate_squire_grades(
        metadata.has_replay,
        &idle_stats,
        &housing_stats,
        metadata.duration_sec,
    );

    Ok(MatchAnalysis {
        merged,
        calibration,
        metadata,
        idle: idle_stats,
        housing: housing_stats,
        floating: floating_stats,
        grades,
    })
}

fn telemetry_only_metadata(merged: &[Observation]) -> MatchMetadata {
    let start_time = if let Some(first_row) = merged.first() {
        if let Some(ts_rw) = first_row.reading().map(|r| r.timestamp_ms as i64) {
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
        (last.in_game_ms() - first.in_game_ms()) as f64 / 1000.0
    } else {
        0.0
    };

    let minutes = (duration_sec / 60.0).floor() as u32;
    let remaining_seconds = duration_sec % 60.0;
    let duration_formatted = format!("{:02}:{:06.3}", minutes, remaining_seconds);

    MatchMetadata {
        start_time,
        duration_formatted,
        duration_sec,
        players: vec![PlayerInfo {
            name: "Active Player".to_string(),
            civ: "Unknown Civ".to_string(),
            is_winner: false,
        }],
        rec_player_name: "Active Player".to_string(),
        rec_player_civ: "Unknown Civ".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        has_replay: false,
        map_name: "Unknown Map".to_string(),
    }
}
