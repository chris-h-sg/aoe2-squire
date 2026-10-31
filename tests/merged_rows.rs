//! The merged timeline written to `output/merged_observations.csv` (MergedRow) and read
//! back by the single-analyzer tools must give the analyzers exactly the same input.
mod common;

use aoe2_squire::analysis::{floating, housing, idle};
use aoe2_squire::report;
use aoe2_squire::types::{MergedRow, Observation};

#[test]
fn merged_csv_round_trip_keeps_analysis_identical() {
    let csv = common::test_bench("1v1-RM-sample.csv");
    let replay = common::test_bench("1v1-RM-sample.aoe2record");
    let direct = common::analyze(&csv, Some(&replay))
        .expect("analysis failed")
        .merged;

    let rows: Vec<MergedRow> = direct.iter().map(MergedRow::from).collect();
    let read_back: Vec<Observation> = rows
        .iter()
        .map(|r| Observation::from_merged_row(r).expect("unreadable merged row"))
        .collect();
    let rows_again: Vec<MergedRow> = read_back.iter().map(MergedRow::from).collect();
    assert_eq!(
        rows, rows_again,
        "MergedRow -> Observation -> MergedRow changed a row"
    );

    let json = |timeline: &[Observation]| {
        let fl = floating::analyze_floating(timeline, false).unwrap();
        serde_json::json!({
            "idle": idle::analyze_idle(timeline, false).unwrap(),
            "housing": housing::analyze_housing(timeline, false).unwrap(),
            "chart": report::extract_chart_data(timeline, &fl),
            "floating": fl,
        })
    };
    assert_eq!(json(&direct), json(&read_back));
}
