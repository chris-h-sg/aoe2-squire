use crate::analysis::{calculate_vs_lost, format_time, segment_game, SegmentEndReason};
use crate::constants::GAME_AGES;
use crate::types::MergedRow;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct IdleSegmentData {
    pub start_formatted: String,
    pub end_formatted: String,
    pub duration_sec: f64,
    pub idle_vils: u32,
    pub vs_lost: f64,
    pub annotation: String,
}

#[derive(Debug, Serialize)]
pub struct IdleReport {
    pub segments: Vec<IdleSegmentData>,
    pub age_vs_lost: HashMap<String, f64>,
    pub total_vs_lost: f64,
}

pub fn analyze_idle(
    rows: impl Iterator<Item = MergedRow>,
    verbose: bool,
) -> Result<IdleReport, Box<dyn Error>> {
    println!("\n=== IDLE VILLAGER ANALYSIS ===");

    let segments = segment_game(rows, |r| r.idle_vils.parse::<u32>().ok());

    let mut report_segments = Vec::new();

    if verbose {
        // 1. Chronological Table
        println!(
            "{:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
            "Start", "End", "Duration", "Idle Vils", "VS Lost"
        );
        println!("{}", "-".repeat(60));
    }

    for seg in &segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let vs_lost = duration_sec * seg.value as f64;

        let annotation = match &seg.end_reason {
            SegmentEndReason::AgeResearch(age) => format!(" ({} Click)", age),
            _ => "".to_string(),
        };

        let start_formatted = format_time(seg.start_ms);
        let end_formatted = format_time(seg.end_ms);

        report_segments.push(IdleSegmentData {
            start_formatted: start_formatted.clone(),
            end_formatted: end_formatted.clone(),
            duration_sec,
            idle_vils: seg.value,
            vs_lost,
            annotation: annotation.clone(),
        });

        if verbose {
            println!(
                "{:>10} | {:>10} | {:>9.1}s | {:>10} | {:>9.1}{}",
                start_formatted, end_formatted, duration_sec, seg.value, vs_lost, annotation
            );
        }
    }

    if verbose {
        println!("{}", "-".repeat(60));
    }

    // 2. Summary
    if verbose {
        println!("{:<15} | {:>10}", "Age", "Idle Time");
        println!("{}", "-".repeat(28));
    }
    let age_vs_lost = calculate_vs_lost(&segments);
    let total: f64 = age_vs_lost.values().sum();

    if verbose {
        for age in GAME_AGES {
            if let Some(lost) = age_vs_lost.get(age) {
                if *lost > 0.0 || age == "Dark Age" {
                    println!("{:<15} | {:>8.1} VS", age, lost);
                }
            }
        }
        println!("{}", "-".repeat(28));
        println!("{:<15} | {:>8.1} VS", "TOTAL", total);
    }

    Ok(IdleReport {
        segments: report_segments,
        age_vs_lost,
        total_vs_lost: total,
    })
}
