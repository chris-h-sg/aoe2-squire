//! Characterization test for the analysis path: telemetry CSV + replay in, summary out.
//! Regenerate the snapshot after an intended change: `UPDATE_SNAPSHOTS=1 cargo test --test analysis_snapshot`
mod common;

use aoe2_squire::analysis::run::MatchAnalysis;
use serde_json::{json, Value};

fn summarize(a: &MatchAnalysis, first_csv_ts: u64) -> Value {
    let cal = a.calibration.as_ref().map(|c| {
        // Real-world timestamps are stored relative to the first CSV row to stay readable.
        let rel = |ts: f64| ts - first_csv_ts as f64;
        let m = &c.mapping;
        let knots: Vec<Value> = m
            .knots()
            .iter()
            .map(|k| json!({ "rw_offset_ms": rel(k.rw_ms), "ig_ms": k.ig_ms }))
            .collect();
        json!({
            "source": c.source,
            "game_start_rw_offset_ms": rel(m.game_start_rw_ms()),
            "speed_at_start": m.speed_at(0.0),
            "segments": m.segments(),
            "pauses": m.pauses().len(),
            "knots": knots,
            "quality": c.quality,
        })
    });

    let m = &a.metadata;
    let screen_grabs = a.merged.iter().filter(|r| r.reading().is_some()).count();
    json!({
        // start_time is rendered in the local timezone and app_version changes every release.
        "metadata": {
            "rec_player_name": m.rec_player_name,
            "rec_player_civ": m.rec_player_civ,
            "map_name": m.map_name,
            "duration_sec": m.duration_sec,
            "duration_formatted": m.duration_formatted,
            "has_replay": m.has_replay,
            "players": m.players,
        },
        "calibration": cal,
        "merged": {
            "rows": a.merged.len(),
            "screen_grabs": screen_grabs,
            "rec_events": a.merged.len() - screen_grabs,
            "first_in_game_ms": a.merged.first().map(|r| r.in_game_ms()),
            "last_in_game_ms": a.merged.last().map(|r| r.in_game_ms()),
        },
        "idle": {
            "age_vs_lost": a.idle.age_vs_lost,
            "total_vs_lost": a.idle.total_vs_lost,
            "total_idle_duration_sec": a.idle.total_idle_duration_sec,
            "total_vs_overall": a.idle.total_vs_overall,
            "segments": a.idle.segments.len(),
        },
        "housing": {
            "metrics": a.housing.metrics,
            "total_housed_sec": a.housing.total_housed_sec,
            "total_queued_sec": a.housing.total_queued_sec,
            "recorded_duration_sec": a.housing.recorded_duration_sec,
            "segments": a.housing.segments.len(),
        },
        "floating": {
            "summary": a.floating.summary,
            "total": a.floating.total,
            "segments": a.floating.segments,
        },
        "grades": a.grades,
    })
}

#[test]
fn analysis_snapshot_1v1_rm_sample() {
    assert_snapshot("1v1-RM-sample");
}

#[test]
fn analysis_snapshot_game1_castle_vs_ai() {
    assert_snapshot("game1-castle-vs-ai");
}

/// Compares the analysis of `test_bench/<game>.csv` + `.aoe2record` with
/// `test_bench/snapshots/<game>.analysis.json`.
fn assert_snapshot(game: &str) {
    let csv = common::test_bench(&format!("{}.csv", game));
    let replay = common::test_bench(&format!("{}.aoe2record", game));
    let snapshot = format!("test_bench/snapshots/{}.analysis.json", game);
    let analysis = common::analyze(&csv, Some(&replay)).expect("analysis failed");

    let first_csv_ts: u64 = std::fs::read_to_string(&csv)
        .unwrap()
        .lines()
        .nth(1)
        .and_then(|l| l.split(',').next())
        .and_then(|ts| ts.parse().ok())
        .expect("first CSV timestamp");
    let actual = summarize(&analysis, first_csv_ts);

    let path = common::root().join(&snapshot);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
        eprintln!("Snapshot written to {}", path.display());
        return;
    }

    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {} (run with UPDATE_SNAPSHOTS=1)", snapshot, e)),
    )
    .unwrap();
    let diffs = common::json_diff(&expected, &actual, 1e-6, 1e-9);
    assert!(
        diffs.is_empty(),
        "Analysis summary differs from {} (if intended, regenerate with UPDATE_SNAPSHOTS=1):\n{}",
        snapshot,
        diffs.join("\n")
    );
}
