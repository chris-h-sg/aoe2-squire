use rts_analyzer::analysis::{
    calculate_housing_metrics, format_time, load_merged_observations, segment_game,
    SegmentEndReason,
};
use rts_analyzer::constants::GAME_AGES;
use rts_analyzer::types::MergedRow;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut rdr = load_merged_observations()?;

    // 1. Cronological Table
    println!(
        "{:>10} | {:>10} | {:>10} | {:>10}",
        "Start", "End", "Duration", "Status"
    );
    println!("{}", "-".repeat(50));

    let rows = rdr
        .deserialize()
        .filter_map(|r: Result<MergedRow, _>| r.ok());
    let segments = segment_game(rows, |r| {
        if r.housing.is_empty() {
            None
        } else {
            Some(r.housing.clone())
        }
    });

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

    // 2. Summary
    let metrics = calculate_housing_metrics(&segments);
    println!("{}", "-".repeat(50));

    let mut total_housed = 0.0;
    let mut total_queued = 0.0;

    for age in GAME_AGES {
        if let Some(age_metrics) = metrics.get(age) {
            let housed = *age_metrics.get("housed").unwrap_or(&0.0);
            let queued = *age_metrics.get("queued").unwrap_or(&0.0);

            if housed > 0.0 || queued > 0.0 || age == "Dark Age" {
                println!(
                    "{:<15}: Housed: {:>6.1}s, Queued: {:>6.1}s",
                    age, housed, queued
                );
            }
            total_housed += housed;
            total_queued += queued;
        }
    }

    println!("{}", "-".repeat(50));
    println!(
        "{:<15}: Housed: {:>6.1}s, Queued: {:>6.1}s",
        "TOTAL", total_housed, total_queued
    );

    Ok(())
}
