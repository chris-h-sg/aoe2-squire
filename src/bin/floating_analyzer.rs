use rts_analyzer::analysis::{format_time, load_merged_observations};
use rts_analyzer::constants::{FLOATING_MIN_DURATION_MS, FLOATING_THRESHOLDS, GAME_AGES};
use rts_analyzer::types::MergedRow;
use std::collections::HashMap;
use std::error::Error;

// --- Core types ---

#[derive(Debug, Default, PartialEq)]
struct FloatingSeconds {
    food: f64,
    wood: f64,
    gold: f64,
    stone: f64,
}

struct FloatingSegment {
    start_ms: u64,
    end_ms: u64,
    resource: String,
    age: String,
}

// --- Analysis logic ---

/// Walk the merged observation rows and compute how many seconds each resource
/// exceeded its age-specific threshold continuously for at least
/// `FLOATING_MIN_DURATION_MS`.
fn calculate_floating_seconds(
    rows: impl Iterator<Item = MergedRow>,
) -> (HashMap<String, FloatingSeconds>, Vec<FloatingSegment>) {
    let mut summary: HashMap<String, FloatingSeconds> = HashMap::new();
    let mut segments: Vec<FloatingSegment> = Vec::new();

    // Tracks the start of the current "above-threshold" run for each resource.
    let mut food_run_start: Option<u64> = None;
    let mut wood_run_start: Option<u64> = None;
    let mut gold_run_start: Option<u64> = None;
    let mut stone_run_start: Option<u64> = None;

    let mut current_age = "Dark Age".to_string();
    let mut age_index: usize = 0;
    let mut last_ts = 0;

    macro_rules! close_run {
        ($run_start:expr, $end_ts:expr, $res_name:expr, $summary_field:expr) => {
            if let Some(start) = $run_start.take() {
                let duration = $end_ts.saturating_sub(start);
                if duration >= FLOATING_MIN_DURATION_MS {
                    $summary_field += duration as f64 / 1000.0;
                    segments.push(FloatingSegment {
                        start_ms: start,
                        end_ms: $end_ts,
                        resource: $res_name.to_string(),
                        age: current_age.clone(),
                    });
                }
            }
        };
    }

    for row in rows {
        last_ts = row.in_game_ms;

        if row.observation_type == "RecEvent" {
            let new_age = if row.event_desc == "Research Feudal Age" {
                Some(("Feudal Age", 1usize))
            } else if row.event_desc == "Research Castle Age" {
                Some(("Castle Age", 2usize))
            } else if row.event_desc == "Research Imperial Age" {
                Some(("Imperial Age", 3usize))
            } else {
                None
            };

            if let Some((name, idx)) = new_age {
                // Before transitioning, close any runs that qualify in the OLD age
                let bucket = summary.entry(current_age.clone()).or_default();
                let ts = row.in_game_ms;
                close_run!(food_run_start, ts, "Food", bucket.food);
                close_run!(wood_run_start, ts, "Wood", bucket.wood);
                close_run!(gold_run_start, ts, "Gold", bucket.gold);
                close_run!(stone_run_start, ts, "Stone", bucket.stone);

                current_age = name.to_string();
                age_index = idx;
            }
            continue;
        }

        if row.observation_type != "ScreenGrab" {
            continue;
        }

        let ts = row.in_game_ms;
        let thresholds = &FLOATING_THRESHOLDS[age_index];

        macro_rules! update_resource {
            ($value_field:expr, $threshold:expr, $run_start:expr, $res_name:expr, $summary_field:expr) => {
                if let Some(val) = $value_field {
                    if val > $threshold {
                        if $run_start.is_none() {
                            $run_start = Some(ts);
                        }
                    } else {
                        close_run!($run_start, ts, $res_name, $summary_field);
                    }
                }
            };
        }

        let food_val = row.food.parse::<u32>().ok();
        let wood_val = row.wood.parse::<u32>().ok();
        let gold_val = row.gold.parse::<u32>().ok();
        let stone_val = row.stone.parse::<u32>().ok();

        let bucket = summary.entry(current_age.clone()).or_default();
        update_resource!(food_val, thresholds.food, food_run_start, "Food", bucket.food);
        update_resource!(wood_val, thresholds.wood, wood_run_start, "Wood", bucket.wood);
        update_resource!(gold_val, thresholds.gold, gold_run_start, "Gold", bucket.gold);
        update_resource!(stone_val, thresholds.stone, stone_run_start, "Stone", bucket.stone);
    }

    // Close any runs that were still active at the end of the telemetry
    let bucket = summary.entry(current_age.clone()).or_default();
    close_run!(food_run_start, last_ts, "Food", bucket.food);
    close_run!(wood_run_start, last_ts, "Wood", bucket.wood);
    close_run!(gold_run_start, last_ts, "Gold", bucket.gold);
    close_run!(stone_run_start, last_ts, "Stone", bucket.stone);

    (summary, segments)
}

// --- Entry point ---

fn main() -> Result<(), Box<dyn Error>> {
    let mut rdr = load_merged_observations()?;

    let rows = rdr
        .deserialize()
        .filter_map(|r: Result<MergedRow, _>| r.ok());

    let (summary, segments) = calculate_floating_seconds(rows);

    // 1. Chronological Log
    println!(
        "{:>12} | {:>12} | {:>10} | {:>10} | {:>10} | {:>10} | {:<15}",
        "Start", "End", "Start ms", "End ms", "Duration", "Resource", "Age"
    );
    println!("{}", "-".repeat(95));

    for seg in &segments {
        let duration_s = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        println!(
            "{:>12} | {:>12} | {:>10} | {:>10} | {:>9.1}s | {:>10} | {:<15}",
            format_time(seg.start_ms),
            format_time(seg.end_ms),
            seg.start_ms,
            seg.end_ms,
            duration_s,
            seg.resource,
            seg.age
        );
    }
    println!("\n");

    // 2. Summary Table
    println!(
        "{:<15} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Age", "Food", "Wood", "Gold", "Stone"
    );
    println!("{}", "-".repeat(65));

    let mut total = FloatingSeconds::default();

    for age in GAME_AGES {
        if let Some(s) = summary.get(age) {
            println!(
                "{:<15} | {:>9.1}s | {:>9.1}s | {:>9.1}s | {:>9.1}s",
                age, s.food, s.wood, s.gold, s.stone
            );
            total.food += s.food;
            total.wood += s.wood;
            total.gold += s.gold;
            total.stone += s.stone;
        }
    }

    println!("{}", "-".repeat(65));
    println!(
        "{:<15} | {:>9.1}s | {:>9.1}s | {:>9.1}s | {:>9.1}s",
        "TOTAL", total.food, total.wood, total.gold, total.stone
    );

    Ok(())
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn grab(ms: u64, food: u32, wood: u32, gold: u32, stone: u32) -> MergedRow {
        MergedRow {
            in_game_ms: ms,
            observation_type: "ScreenGrab".to_string(),
            food: food.to_string(),
            wood: wood.to_string(),
            gold: gold.to_string(),
            stone: stone.to_string(),
            ..Default::default()
        }
    }

    fn age_event(ms: u64, desc: &str) -> MergedRow {
        MergedRow {
            in_game_ms: ms,
            observation_type: "RecEvent".to_string(),
            event_desc: desc.to_string(),
            ..Default::default()
        }
    }

    /// A single resource (wood) floats for 40s in Dark Age — should be flagged.
    #[test]
    fn test_floating_basic_single_resource() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),
            grab(40_000, 100, 100, 50, 100),
        ];
        let (summary, _) = calculate_floating_seconds(rows.into_iter());
        let dark = summary.get("Dark Age").unwrap();
        assert_eq!(dark.wood, 40.0);
    }

    /// Tests that the final run is closed even if the value never drops.
    #[test]
    fn test_floating_closed_at_end() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),
            grab(40_000, 100, 300, 50, 100),
        ];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter());
        let dark = summary.get("Dark Age").unwrap();
        assert_eq!(dark.wood, 40.0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 40000);
    }

    #[test]
    fn test_floating_exact_threshold_not_flagged() {
        let rows = vec![grab(0, 100, 200, 50, 100), grab(60_000, 100, 200, 50, 100)];
        let (summary, _) = calculate_floating_seconds(rows.into_iter());
        let dark = summary.get("Dark Age").unwrap();
        assert_eq!(dark.wood, 0.0);
    }

    #[test]
    fn test_floating_below_minimum_duration_not_flagged() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),
            grab(29_000, 100, 100, 50, 100),
        ];
        let (summary, _) = calculate_floating_seconds(rows.into_iter());
        let dark = summary.get("Dark Age").unwrap();
        assert_eq!(dark.wood, 0.0);
    }

    #[test]
    fn test_floating_cross_age_attribution() {
        let rows = vec![
            grab(0, 300, 100, 50, 100),
            grab(60_000, 100, 100, 50, 100),
            age_event(70_000, "Research Feudal Age"),
            grab(70_000, 100, 100, 400, 100),
            grab(115_000, 100, 100, 100, 100),
        ];
        let (summary, _) = calculate_floating_seconds(rows.into_iter());
        assert_eq!(summary.get("Dark Age").unwrap().food, 60.0);
        assert_eq!(summary.get("Feudal Age").unwrap().gold, 45.0);
    }

    /// Tests that runs are CLOSED (not discarded) at age transitions.
    #[test]
    fn test_floating_run_closed_on_age_transition() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),       // Dark: wood=300 > 200 — run starts
            age_event(40_000, "Research Feudal Age"), // transition closes run (40s)
            grab(40_000, 100, 600, 50, 100),  // Feudal: wood=600 > 500 — new run starts
            grab(80_000, 100, 100, 50, 100),  // Feudal: wood drops (40s run)
        ];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter());
        
        assert_eq!(summary.get("Dark Age").unwrap().wood, 40.0);
        assert_eq!(summary.get("Feudal Age").unwrap().wood, 40.0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].age, "Dark Age");
        assert_eq!(segments[1].age, "Feudal Age");
    }

    /// Tests that a run is IGNORED at age transition if it hasn't reached 30s yet.
    #[test]
    fn test_floating_run_ignored_if_short_at_transition() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),       // Dark: wood=300 > 200 — run starts
            age_event(20_000, "Research Feudal Age"), // transition at 20s — too short!
            grab(20_000, 100, 100, 50, 100),
        ];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter());
        
        assert_eq!(summary.get("Dark Age").unwrap().wood, 0.0);
        assert!(segments.is_empty());
    }
}
