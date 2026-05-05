use crate::analysis::{
    calculate_vs_lost, format_time, segment_game, SegmentEndReason,
};
use crate::constants::GAME_AGES;
use crate::types::MergedRow;
use std::error::Error;

pub fn analyze_idle(
    rows: impl Iterator<Item = MergedRow>,
    verbose: bool,
) -> Result<(), Box<dyn Error>> {
    println!("\n=== IDLE VILLAGER ANALYSIS ===");

    let segments = segment_game(rows, |r| r.idle_vils.parse::<u32>().ok());

    if verbose {
        // 1. Chronological Table
        println!(
            "{:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
            "Start", "End", "Duration", "Idle Vils", "VS Lost"
        );
        println!("{}", "-".repeat(60));

        for seg in &segments {
            let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
            let vs_lost = duration_sec * seg.value as f64;

            let annotation = match &seg.end_reason {
                SegmentEndReason::AgeResearch(age) => format!(" ({} Click)", age),
                _ => "".to_string(),
            };

            println!(
                "{:>10} | {:>10} | {:>9.1}s | {:>10} | {:>9.1}{}",
                format_time(seg.start_ms),
                format_time(seg.end_ms),
                duration_sec,
                seg.value,
                vs_lost,
                annotation
            );
        }
        println!("{}", "-".repeat(60));
    }

    // 2. Summary
    println!("{:<15} | {:>10}", "Age", "Idle Time");
    println!("{}", "-".repeat(28));
    let age_vs_lost = calculate_vs_lost(&segments);
    let total: f64 = age_vs_lost.values().sum();

    for age in GAME_AGES {
        if let Some(lost) = age_vs_lost.get(age) {
            if *lost > 0.0 || age == "Dark Age" {
                println!("{:<15} | {:>8.1} VS", age, lost);
            }
        }
    }
    println!("{}", "-".repeat(28));
    println!("{:<15} | {:>8.1} VS", "TOTAL", total);

    Ok(())
}
