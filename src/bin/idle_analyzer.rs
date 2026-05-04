use rts_analyzer::analysis::{
    calculate_vs_lost, format_time, load_merged_observations, segment_game, SegmentEndReason,
};
use rts_analyzer::constants::GAME_AGES;
use rts_analyzer::types::MergedRow;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut rdr = load_merged_observations()?;

    // 1. Chronological Table
    println!(
        "{:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Start", "End", "Duration", "Idle Vils", "VS Lost"
    );
    println!("{}", "-".repeat(60));

    let rows = rdr
        .deserialize()
        .filter_map(|r: Result<MergedRow, _>| r.ok());
    let segments = segment_game(rows, |r| r.idle_vils.parse::<u32>().ok());

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

    // 2. Summary
    let age_vs_lost = calculate_vs_lost(&segments);
    println!("{}", "-".repeat(60));
    let total: f64 = age_vs_lost.values().sum();

    for age in GAME_AGES {
        if let Some(lost) = age_vs_lost.get(age) {
            if *lost > 0.0 || age == "Dark Age" {
                println!("{:<15}: {:>8.1} VS", age, lost);
            }
        }
    }
    println!("{:<15}: {:>8.1} VS", "TOTAL", total);

    Ok(())
}
