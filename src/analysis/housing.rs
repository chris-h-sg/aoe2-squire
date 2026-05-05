use crate::analysis::{
    calculate_housing_metrics, format_time, segment_game, SegmentEndReason,
};
use crate::constants::GAME_AGES;
use crate::types::MergedRow;
use std::error::Error;

pub fn analyze_housing(
    rows: impl Iterator<Item = MergedRow>,
    verbose: bool,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== HOUSING EFFICIENCY ANALYSIS ===");

    let segments = segment_game(rows, |r| {
        if r.housing.is_empty() {
            None
        } else {
            Some(r.housing.clone())
        }
    });

    if verbose {
        // 1. Cronological Table
        println!(
            "{:>10} | {:>10} | {:>10} | {:>10}",
            "Start", "End", "Duration", "Status"
        );
        println!("{}", "-".repeat(50));

        for seg in &segments {
            let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
            let annotation = match &seg.end_reason {
                SegmentEndReason::AgeResearch(age) => format!(" ({} Click)", age),
                _ => "".to_string(),
            };

            println!(
                "{:>10} | {:>10} | {:>9.1}s | {:>10}{}",
                format_time(seg.start_ms),
                format_time(seg.end_ms),
                duration_sec,
                seg.value,
                annotation
            );
        }
        println!("{}", "-".repeat(50));
    }

    // 2. Summary
    let metrics = calculate_housing_metrics(&segments);
    println!("{:<15} | {:>10} | {:>12}", "Age", "Housed", "Fully Queued");
    println!("{}", "-".repeat(43));

    let mut total_housed = 0.0;
    let mut total_queued = 0.0;

    for age in GAME_AGES {
        if let Some(age_metrics) = metrics.get(age) {
            let housed = *age_metrics.get("housed").unwrap_or(&0.0);
            let queued = *age_metrics.get("queued").unwrap_or(&0.0);

            if housed > 0.0 || queued > 0.0 || age == "Dark Age" {
                println!(
                    "{:<15} | {:>9.1}s | {:>11.1}s",
                    age, housed, queued
                );
            }
            total_housed += housed;
            total_queued += queued;
        }
    }

    println!("{}", "-".repeat(43));
    println!(
        "{:<15} | {:>9.1}s | {:>11.1}s",
        "TOTAL", total_housed, total_queued
    );

    Ok(())
}
