//! The sanity report (src/validation.rs) on the sample game, and a check that its spend
//! alignment score actually notices broken sync (scenarios from tests/sync_stress.rs).
//!
//! Every finding on the sample game has been reviewed and explained in REVIEWED_FINDINGS;
//! the test fails on any new, vanished or changed finding, so after a change only the
//! difference needs a look. After reviewing, `UPDATE_SNAPSHOTS=1 cargo test --test
//! sanity_report` rewrites the file, keeping existing explanations; new findings get an
//! empty explanation that must be filled in for the test to pass.
mod common;

use aoe2_squire::analysis::format_time;
use aoe2_squire::validation::{self, Finding, SanityReport};
use common::Csv;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const REVIEWED_FINDINGS: &str = "test_bench/snapshots/1v1-RM-sample.sanity.json";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Summary {
    matched: usize,
    total: usize,
    farm_reseeds: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Reviewed {
    section: String,
    time: String,
    field: String,
    detail: String,
    #[serde(default)]
    explanation: String,
}

impl Reviewed {
    fn key(&self) -> (String, String, String) {
        (self.section.clone(), self.time.clone(), self.field.clone())
    }
}

#[derive(Serialize, Deserialize)]
struct ReviewedReport {
    spend_alignment: Summary,
    findings: Vec<Reviewed>,
}

fn reviewable(r: &SanityReport) -> ReviewedReport {
    let mut findings = vec![];
    let mut add = |section: &str, list: &[Finding]| {
        findings.extend(list.iter().map(|f| Reviewed {
            section: section.to_string(),
            time: format_time(f.in_game_ms),
            field: f.field.clone(),
            detail: f.detail.clone(),
            explanation: String::new(),
        }))
    };
    let s = r.spends.as_ref().expect("spend alignment");
    add("unmatched_spends", &s.unmatched);
    add("overclaimed", &s.overclaimed);
    add("unexplained_drops", &s.unexplained_drops);
    add("glitches", &r.glitches);
    add("invariant_violations", &r.invariant_violations);
    add("large_increases", &r.large_increases);
    ReviewedReport {
        spend_alignment: Summary {
            matched: s.matched,
            total: s.total,
            farm_reseeds: s.farm_reseeds,
        },
        findings,
    }
}

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

#[test]
fn sample_game_findings_are_all_reviewed() {
    let mut actual = reviewable(&report(&Csv::load(), "sanity_sample_reviewed"));
    let path = common::root().join(REVIEWED_FINDINGS);
    let saved: Option<ReviewedReport> = std::fs::read_to_string(&path)
        .ok()
        .map(|s| serde_json::from_str(&s).expect("invalid reviewed findings file"));
    let explanations: BTreeMap<_, _> = saved
        .iter()
        .flat_map(|s| &s.findings)
        .map(|f| (f.key(), f.explanation.clone()))
        .collect();

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        for f in &mut actual.findings {
            f.explanation = explanations.get(&f.key()).cloned().unwrap_or_default();
        }
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
        eprintln!("Reviewed findings written to {}", path.display());
    }
    let saved =
        saved.unwrap_or_else(|| panic!("{} missing (UPDATE_SNAPSHOTS=1)", REVIEWED_FINDINGS));

    let mut problems = vec![];
    if actual.spend_alignment != saved.spend_alignment {
        problems.push(format!(
            "spend alignment changed: {:?} -> {:?}",
            saved.spend_alignment, actual.spend_alignment
        ));
    }
    let saved_by_key: BTreeMap<_, _> = saved.findings.iter().map(|f| (f.key(), f)).collect();
    let actual_by_key: BTreeMap<_, _> = actual.findings.iter().map(|f| (f.key(), f)).collect();
    for (key, f) in &actual_by_key {
        match saved_by_key.get(key) {
            None => problems.push(format!(
                "new: {} {} {}: {}",
                f.section, f.time, f.field, f.detail
            )),
            Some(s) if s.detail != f.detail => problems.push(format!(
                "changed: {} {} {}: {} -> {}",
                f.section, f.time, f.field, s.detail, f.detail
            )),
            _ => {}
        }
    }
    for (key, s) in &saved_by_key {
        if !actual_by_key.contains_key(key) {
            problems.push(format!(
                "gone: {} {} {}: {} (was: {})",
                s.section, s.time, s.field, s.detail, s.explanation
            ));
        }
    }
    for s in saved
        .findings
        .iter()
        .filter(|s| s.explanation.trim().is_empty())
    {
        problems.push(format!(
            "not explained yet: {} {} {}: {}",
            s.section, s.time, s.field, s.detail
        ));
    }
    assert!(
        problems.is_empty(),
        "Sanity findings on the sample game differ from {} — review them, then update the \
         file (UPDATE_SNAPSHOTS=1 keeps existing explanations):\n  {}",
        REVIEWED_FINDINGS,
        problems.join("\n  ")
    );
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
