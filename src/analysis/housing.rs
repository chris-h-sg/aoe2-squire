use crate::analysis::{calculate_housing_metrics, format_time, segment_game, SegmentEndReason};
use crate::constants::GAME_AGES;
use crate::types::MergedRow;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct HousingSegmentData {
    pub start_formatted: String,
    pub end_formatted: String,
    pub duration_sec: f64,
    pub status: String,
    pub annotation: String,
}

#[derive(Debug, Serialize)]
pub struct HousingReport {
    pub segments: Vec<HousingSegmentData>,
    pub metrics: HashMap<String, HashMap<String, f64>>,
    pub total_housed_sec: f64,
    pub total_queued_sec: f64,
    pub recorded_duration_sec: f64,
}

pub fn analyze_housing(
    rows: impl Iterator<Item = MergedRow>,
    verbose: bool,
) -> Result<HousingReport, Box<dyn Error>> {
    println!("\n=== HOUSING EFFICIENCY ANALYSIS ===");

    let segments = segment_game(rows, |r| {
        if r.housing.is_empty() {
            None
        } else {
            Some(r.housing.clone())
        }
    });

    let mut report_segments = Vec::new();

    if verbose {
        // 1. Cronological Table
        println!(
            "{:>10} | {:>10} | {:>10} | {:>10}",
            "Start", "End", "Duration", "Status"
        );
        println!("{}", "-".repeat(50));
    }

    for seg in &segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let annotation = match &seg.end_reason {
            SegmentEndReason::AgeResearch(age) => format!(" ({} Click)", age),
            _ => "".to_string(),
        };

        let start_formatted = format_time(seg.start_ms);
        let end_formatted = format_time(seg.end_ms);

        report_segments.push(HousingSegmentData {
            start_formatted: start_formatted.clone(),
            end_formatted: end_formatted.clone(),
            duration_sec,
            status: seg.value.clone(),
            annotation: annotation.clone(),
        });

        if verbose {
            println!(
                "{:>10} | {:>10} | {:>9.1}s | {:>10}{}",
                start_formatted, end_formatted, duration_sec, seg.value, annotation
            );
        }
    }

    if verbose {
        println!("{}", "-".repeat(50));
    }

    // 2. Summary
    let metrics = calculate_housing_metrics(&segments);

    if verbose {
        println!("{:<15} | {:>10} | {:>12}", "Age", "Housed", "Fully Queued");
        println!("{}", "-".repeat(43));
    }

    let mut total_housed = 0.0;
    let mut total_queued = 0.0;

    for age in GAME_AGES {
        if let Some(age_metrics) = metrics.get(age) {
            let housed = *age_metrics.get("housed").unwrap_or(&0.0);
            let queued = *age_metrics.get("queued").unwrap_or(&0.0);

            if verbose && (housed > 0.0 || queued > 0.0 || age == "Dark Age") {
                println!("{:<15} | {:>9.1}s | {:>11.1}s", age, housed, queued);
            }
            total_housed += housed;
            total_queued += queued;
        }
    }

    if verbose {
        println!("{}", "-".repeat(43));
        println!(
            "{:<15} | {:>9.1}s | {:>11.1}s",
            "TOTAL", total_housed, total_queued
        );
    }

    let recorded_duration_sec = report_segments.iter().map(|s| s.duration_sec).sum::<f64>();

    Ok(HousingReport {
        segments: report_segments,
        metrics,
        total_housed_sec: total_housed,
        total_queued_sec: total_queued,
        recorded_duration_sec,
    })
}
