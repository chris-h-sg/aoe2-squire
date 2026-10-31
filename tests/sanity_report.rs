//! The sanity report (src/validation.rs) on the sample game, and a check that its spend
//! alignment score actually notices broken sync (scenarios from tests/sync_stress.rs).
mod common;

use aoe2_squire::validation::{self, SanityReport};
use common::Csv;

/// 188 of 189 spends match on the sample game; the one left is a house placement that
/// never happened on screen.
const SAMPLE_MIN_SCORE: f64 = 0.99;
/// Broken sync must push the score clearly below that.
const BROKEN_SYNC_MAX_SCORE: f64 = 0.8;
/// Real-world offsets from the first CSV row: between the two sync anchors (first spend
/// at 2.5 s, Feudal click at 285 s), and a minute after Feudal.
const BEFORE_FEUDAL_MS: u64 = 144_000;
const AFTER_FEUDAL_MS: u64 = 345_000;

fn report(csv: &Csv, name: &str) -> SanityReport {
    let replay = common::test_bench("1v1-RM-sample.aoe2record");
    let analysis = common::analyze(&csv.write(name), Some(&replay)).expect("analysis failed");
    let report = validation::check(&analysis.merged);
    println!("{}:\n{}", name, report.to_text());
    report
}

#[test]
fn sample_game_is_consistent() {
    let r = report(&Csv::load(), "sanity_sample");
    let s = r.spends.as_ref().expect("spend alignment");
    assert!(
        s.score >= SAMPLE_MIN_SCORE,
        "spend alignment {:.3} below {}",
        s.score,
        SAMPLE_MIN_SCORE
    );
    // A drop shows within a frame or two of its replay event, all game long.
    for q in s.offset_by_quarter_ms.iter().flatten() {
        assert!((0..=600).contains(q), "median offset {} ms", q);
    }
    assert_eq!(r.missing.values().sum::<usize>(), 0);
}

fn assert_detected(name: &str, csv: &Csv) {
    let r = report(csv, name);
    let score = r.spends.expect("spend alignment").score;
    assert!(
        score <= BROKEN_SYNC_MAX_SCORE,
        "{}: broken sync not detected, spend alignment {:.3}",
        name,
        score
    );
}

#[test]
fn detects_speed_2_0() {
    let mut csv = Csv::load();
    csv.stretch(1.69 / 2.0);
    assert_detected("sanity_speed_2_0", &csv);
}

#[test]
fn detects_pause_before_feudal() {
    let mut csv = Csv::load();
    let i = csv.index_at(csv.first_ts() + BEFORE_FEUDAL_MS);
    csv.pause(i, 8_000);
    assert_detected("sanity_pause_8s_before_feudal", &csv);
}

#[test]
fn detects_pause_after_feudal() {
    let mut csv = Csv::load();
    let i = csv.index_at(csv.first_ts() + AFTER_FEUDAL_MS);
    csv.pause(i, 30_000);
    assert_detected("sanity_pause_30s_after_feudal", &csv);
}
