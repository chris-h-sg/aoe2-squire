use csv::ReaderBuilder;
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let merged_csv_path = "output/merged_observations.csv";
    if !Path::new(merged_csv_path).exists() {
        eprintln!("Error: {} not found. Run mesher first.", merged_csv_path);
        std::process::exit(1);
    }

    let mut rdr = ReaderBuilder::new().from_path(merged_csv_path)?;

    let mut current_idle_count: Option<u32> = None;
    let mut segment_start_ms: u64 = 0;
    let mut current_age = "Dark Age";

    let mut age_vs_lost = std::collections::HashMap::new();
    age_vs_lost.insert("Dark Age", 0.0);
    age_vs_lost.insert("Feudal Age", 0.0);
    age_vs_lost.insert("Castle Age", 0.0);
    age_vs_lost.insert("Imperial Age", 0.0);

    println!(
        "{:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Start", "End", "Duration", "Idle Vils", "VS Lost"
    );
    println!("{}", "-".repeat(60));

    for result in rdr.records() {
        let record = result?;
        let ig_ms: u64 = record[0].parse().unwrap_or(0);
        let obs_type = &record[1];

        if obs_type == "ScreenGrab" {
            let idle_vils_str = &record[10];
            if !idle_vils_str.is_empty() {
                let new_idle_count: u32 = idle_vils_str.parse().unwrap_or(0);

                match current_idle_count {
                    Some(old_count) if new_idle_count != old_count => {
                        // End current segment
                        let duration_sec = (ig_ms - segment_start_ms) as f64 / 1000.0;
                        let vs_lost = duration_sec * old_count as f64;
                        *age_vs_lost.entry(current_age).or_insert(0.0) += vs_lost;

                        println!(
                            "{:>10} | {:>10} | {:>9.1}s | {:>10} | {:>9.1}",
                            format_time(segment_start_ms),
                            format_time(ig_ms),
                            duration_sec,
                            old_count,
                            vs_lost
                        );

                        segment_start_ms = ig_ms;
                        current_idle_count = Some(new_idle_count);
                    }
                    None => {
                        // Initialize first segment
                        segment_start_ms = ig_ms;
                        current_idle_count = Some(new_idle_count);
                    }
                    _ => {} // Same idle count, continue segment
                }
            }
        } else if obs_type == "RecEvent" {
            let desc = &record[2];
            let new_age_name = if desc == "Research Feudal Age" {
                Some("Feudal Age")
            } else if desc == "Research Castle Age" {
                Some("Castle Age")
            } else if desc == "Research Imperial Age" {
                Some("Imperial Age")
            } else {
                None
            };

            if let Some(age_name) = new_age_name {
                // Age click ends the current segment for the current age
                if let Some(count) = current_idle_count {
                    let duration_sec = (ig_ms - segment_start_ms) as f64 / 1000.0;
                    let vs_lost = duration_sec * count as f64;
                    *age_vs_lost.entry(current_age).or_insert(0.0) += vs_lost;

                    println!(
                        "{:>10} | {:>10} | {:>9.1}s | {:>10} | {:>9.1} ({} Click)",
                        format_time(segment_start_ms),
                        format_time(ig_ms),
                        duration_sec,
                        count,
                        vs_lost,
                        current_age
                    );
                }

                println!(
                    "--- Transitioning to {} at {} ---",
                    age_name,
                    format_time(ig_ms)
                );
                current_age = age_name;
                segment_start_ms = ig_ms;
            }
        }
    }

    // Handle final segment up to last recorded time
    // (Note: we don't have the absolute end-of-replay here, but we can close the last segment)
    // The last record's ig_ms is available from the loop, but we need to capture it.
    // Let's just assume the loop finished at the last available observation.

    // Calculate final summary using library
    let rdr_for_analysis = ReaderBuilder::new().from_path(merged_csv_path)?;
    let segments = rts_analyzer::analysis::segment_observations(
        rdr_for_analysis.into_records().filter_map(|r| r.ok()),
    );
    let age_vs_lost = rts_analyzer::analysis::calculate_vs_lost(&segments);

    println!("{}", "-".repeat(60));
    let total: f64 = age_vs_lost.values().sum();

    let ages = ["Dark Age", "Feudal Age", "Castle Age", "Imperial Age"];
    for age in ages {
        if let Some(lost) = age_vs_lost.get(age) {
            if *lost > 0.0 || age == "Dark Age" {
                println!("{:<15}: {:>8.1} VS", age, lost);
            }
        }
    }
    println!("{:<15}: {:>8.1} VS", "TOTAL", total);

    Ok(())
}

fn format_time(ms: u64) -> String {
    let minutes = ms / 60000;
    let seconds = (ms % 60000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}.{:03}", minutes, seconds, millis)
}
