use crate::analysis::format_time;
use crate::constants::{
    FLOATING_MIN_DURATION_MS, FLOATING_THRESHOLDS, GAME_AGES, SAVE_UP_WINDOW_MS,
};
use crate::types::MergedRow;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;

// --- Core types ---

#[derive(Debug, Default, PartialEq, Serialize, Clone)]
pub struct FloatingSeconds {
    pub food: f64,
    pub wood: f64,
    pub gold: f64,
    pub stone: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct FloatingSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub resource: String,
    pub age: String,
}

#[derive(Debug, Serialize)]
pub struct FloatingReport {
    pub summary: HashMap<String, FloatingSeconds>,
    pub segments: Vec<FloatingSegment>,
    pub total: FloatingSeconds,
}

/// Which resources to suppress in the 60-second save-up window before a major spend event.
pub struct Suppressions {
    pub food: Vec<(u64, u64)>,
    pub gold: Vec<(u64, u64)>,
    pub stone: Vec<(u64, u64)>,
}

/// Pre-index every major spend event and its per-resource suppression intervals:
/// - Age-up clicks (suppress food / food+gold)
/// - Castle placements (suppress stone)
pub fn index_save_up_events(rows: &[MergedRow]) -> Suppressions {
    let mut food = Vec::new();
    let mut gold = Vec::new();
    let mut stone = Vec::new();

    for r in rows.iter().filter(|r| r.observation_type == "RecEvent") {
        let window_start = r.in_game_ms.saturating_sub(SAVE_UP_WINDOW_MS);
        let window_end = r.in_game_ms;
        match r.event_desc.as_str() {
            "Research Feudal Age" => {
                food.push((window_start, window_end));
            }
            "Research Castle Age" | "Research Imperial Age" => {
                food.push((window_start, window_end));
                gold.push((window_start, window_end));
            }
            "Build Castle" => {
                stone.push((window_start, window_end));
            }
            _ => {}
        }
    }

    food.sort_unstable();
    gold.sort_unstable();
    stone.sort_unstable();

    Suppressions { food, gold, stone }
}

/// Returns the list of valid `(start_ms, end_ms)` intervals after carving out any
/// overlapping save-up windows.
fn apply_suppression(start_ms: u64, end_ms: u64, intervals: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut result = Vec::new();
    let mut current_start = start_ms;

    for &(w_start, w_end) in intervals {
        if current_start >= end_ms {
            break;
        }
        if end_ms > w_start && current_start < w_end {
            if current_start < w_start {
                result.push((current_start, w_start));
            }
            current_start = std::cmp::max(current_start, w_end);
        }
    }

    if current_start < end_ms {
        result.push((current_start, end_ms));
    }

    result
}

// --- Analysis logic ---

/// Walk the merged observation rows and compute how many seconds each resource
/// exceeded its age-specific threshold continuously for at least
/// `FLOATING_MIN_DURATION_MS`, after applying save-up window suppression.
pub fn calculate_floating_seconds(
    rows: impl Iterator<Item = MergedRow>,
    suppressions: &Suppressions,
) -> (HashMap<String, FloatingSeconds>, Vec<FloatingSegment>) {
    let mut summary: HashMap<String, FloatingSeconds> = HashMap::new();
    let mut segments: Vec<FloatingSegment> = Vec::new();

    let mut food_run_start: Option<u64> = None;
    let mut wood_run_start: Option<u64> = None;
    let mut gold_run_start: Option<u64> = None;
    let mut stone_run_start: Option<u64> = None;

    let mut current_age = "Dark Age".to_string();
    let mut age_index: usize = 0;
    let mut last_ts = 0;

    macro_rules! close_run {
        ($run_start:expr, $end_ts:expr, $res_name:expr, $intervals:expr, $summary_field:expr) => {
            if let Some(start) = $run_start.take() {
                for (eff_start, eff_end) in apply_suppression(start, $end_ts, $intervals) {
                    let duration = eff_end.saturating_sub(eff_start);
                    if duration >= FLOATING_MIN_DURATION_MS {
                        $summary_field += duration as f64 / 1000.0;
                        segments.push(FloatingSegment {
                            start_ms: eff_start,
                            end_ms: eff_end,
                            resource: $res_name.to_string(),
                            age: current_age.clone(),
                        });
                    }
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
                let bucket = summary.entry(current_age.clone()).or_default();
                let ts = row.in_game_ms;
                close_run!(food_run_start, ts, "Food", &suppressions.food, bucket.food);
                close_run!(wood_run_start, ts, "Wood", &[], bucket.wood);
                close_run!(gold_run_start, ts, "Gold", &suppressions.gold, bucket.gold);
                close_run!(
                    stone_run_start,
                    ts,
                    "Stone",
                    &suppressions.stone,
                    bucket.stone
                );

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
            ($value_field:expr, $threshold:expr, $run_start:expr, $res_name:expr, $intervals:expr, $summary_field:expr) => {
                if let Some(val) = $value_field {
                    if val > $threshold {
                        if $run_start.is_none() {
                            $run_start = Some(ts);
                        }
                    } else {
                        close_run!($run_start, ts, $res_name, $intervals, $summary_field);
                    }
                }
            };
        }

        let food_val = row.food.parse::<u32>().ok();
        let wood_val = row.wood.parse::<u32>().ok();
        let gold_val = row.gold.parse::<u32>().ok();
        let stone_val = row.stone.parse::<u32>().ok();

        let bucket = summary.entry(current_age.clone()).or_default();
        update_resource!(
            food_val,
            thresholds.food,
            food_run_start,
            "Food",
            &suppressions.food,
            bucket.food
        );
        update_resource!(
            wood_val,
            thresholds.wood,
            wood_run_start,
            "Wood",
            &[],
            bucket.wood
        );
        update_resource!(
            gold_val,
            thresholds.gold,
            gold_run_start,
            "Gold",
            &suppressions.gold,
            bucket.gold
        );
        update_resource!(
            stone_val,
            thresholds.stone,
            stone_run_start,
            "Stone",
            &suppressions.stone,
            bucket.stone
        );
    }

    // Close any runs still active at the end of the telemetry
    let bucket = summary.entry(current_age.clone()).or_default();
    close_run!(
        food_run_start,
        last_ts,
        "Food",
        &suppressions.food,
        bucket.food
    );
    close_run!(wood_run_start, last_ts, "Wood", &[], bucket.wood);
    close_run!(
        gold_run_start,
        last_ts,
        "Gold",
        &suppressions.gold,
        bucket.gold
    );
    close_run!(
        stone_run_start,
        last_ts,
        "Stone",
        &suppressions.stone,
        bucket.stone
    );

    (summary, segments)
}

// --- Entry point ---

pub fn analyze_floating(
    rows: Vec<MergedRow>,
    verbose: bool,
) -> Result<FloatingReport, Box<dyn Error>> {
    println!("\n=== FLOATING RESOURCE ANALYSIS ===");
    let suppressions = index_save_up_events(&rows);
    let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &suppressions);

    if verbose {
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
    }

    // 2. Summary Table
    if verbose {
        println!(
            "{:<15} | {:>10} | {:>10} | {:>10} | {:>10}",
            "Age", "Food", "Wood", "Gold", "Stone"
        );
        println!("{}", "-".repeat(65));
    }

    let mut total = FloatingSeconds::default();

    for age in GAME_AGES {
        if let Some(s) = summary.get(age) {
            if verbose {
                println!(
                    "{:<15} | {:>9.1}s | {:>9.1}s | {:>9.1}s | {:>9.1}s",
                    age, s.food, s.wood, s.gold, s.stone
                );
            }
            total.food += s.food;
            total.wood += s.wood;
            total.gold += s.gold;
            total.stone += s.stone;
        }
    }

    if verbose {
        println!("{}", "-".repeat(65));
        println!(
            "{:<15} | {:>9.1}s | {:>9.1}s | {:>9.1}s | {:>9.1}s",
            "TOTAL", total.food, total.wood, total.gold, total.stone
        );
    }

    Ok(FloatingReport {
        summary,
        segments,
        total,
    })
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MergedRow;

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

    fn no_sup() -> Suppressions {
        Suppressions {
            food: vec![],
            gold: vec![],
            stone: vec![],
        }
    }

    // -------------------------------------------------------------------------
    // Existing behaviour (no suppression)
    // -------------------------------------------------------------------------

    /// A single resource (wood) floats for 40s in Dark Age — should be flagged.
    #[test]
    fn test_floating_basic_single_resource() {
        let rows = vec![grab(0, 100, 300, 50, 100), grab(40_000, 100, 100, 50, 100)];
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        assert_eq!(summary.get("Dark Age").unwrap().wood, 40.0);
    }

    /// Tests that the final run is closed even if the value never drops.
    #[test]
    fn test_floating_closed_at_end() {
        let rows = vec![grab(0, 100, 300, 50, 100), grab(40_000, 100, 300, 50, 100)];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        let dark = summary.get("Dark Age").unwrap();
        assert_eq!(dark.wood, 40.0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 40000);
    }

    #[test]
    fn test_floating_exact_threshold_not_flagged() {
        let rows = vec![grab(0, 100, 200, 50, 100), grab(60_000, 100, 200, 50, 100)];
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        assert_eq!(summary.get("Dark Age").unwrap().wood, 0.0);
    }

    #[test]
    fn test_floating_below_minimum_duration_not_flagged() {
        let rows = vec![grab(0, 100, 300, 50, 100), grab(29_000, 100, 100, 50, 100)];
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        assert_eq!(summary.get("Dark Age").unwrap().wood, 0.0);
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
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        assert_eq!(summary.get("Dark Age").unwrap().food, 60.0);
        assert_eq!(summary.get("Feudal Age").unwrap().gold, 45.0);
    }

    /// Tests that runs are CLOSED (not discarded) at age transitions.
    #[test]
    fn test_floating_run_closed_on_age_transition() {
        let rows = vec![
            grab(0, 100, 300, 50, 100),
            age_event(40_000, "Research Feudal Age"),
            grab(40_000, 100, 600, 50, 100),
            grab(80_000, 100, 100, 50, 100),
        ];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &no_sup());
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
            grab(0, 100, 300, 50, 100),
            age_event(20_000, "Research Feudal Age"),
            grab(20_000, 100, 100, 50, 100),
        ];
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &no_sup());
        assert_eq!(summary.get("Dark Age").unwrap().wood, 0.0);
        assert!(segments.is_empty());
    }

    // -------------------------------------------------------------------------
    // Save-up window suppression
    // -------------------------------------------------------------------------

    fn feudal_sup(click_ms: u64) -> Suppressions {
        Suppressions {
            food: vec![(click_ms.saturating_sub(SAVE_UP_WINDOW_MS), click_ms)],
            gold: vec![],
            stone: vec![],
        }
    }

    fn castle_age_sup(click_ms: u64) -> Suppressions {
        Suppressions {
            food: vec![(click_ms.saturating_sub(SAVE_UP_WINDOW_MS), click_ms)],
            gold: vec![(click_ms.saturating_sub(SAVE_UP_WINDOW_MS), click_ms)],
            stone: vec![],
        }
    }

    fn castle_build_sup(click_ms: u64) -> Suppressions {
        Suppressions {
            food: vec![],
            gold: vec![],
            stone: vec![(click_ms.saturating_sub(SAVE_UP_WINDOW_MS), click_ms)],
        }
    }

    /// Food run entirely inside the 60s feudal save-up window → discarded.
    #[test]
    fn test_suppression_food_feudal_fully_inside_window() {
        let feudal_click = 500_000u64;
        // Run starts 50s before click — fully inside the 60s window
        let rows = vec![
            grab(feudal_click - 50_000, 600, 100, 50, 100),
            grab(feudal_click, 100, 100, 50, 100),
        ];
        let sup = feudal_sup(feudal_click);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        assert_eq!(
            summary
                .get("Dark Age")
                .unwrap_or(&FloatingSeconds::default())
                .food,
            0.0
        );
        assert!(segments.is_empty());
    }

    /// Food run partially overlapping the 60s feudal window — only the pre-window
    /// portion should be counted (90s float, 60s suppressed → 30s counted).
    #[test]
    fn test_suppression_food_feudal_partial_overlap() {
        let feudal_click = 500_000u64;
        // Run starts 90s before click — 30s before window, 60s inside window
        let rows = vec![
            grab(feudal_click - 90_000, 600, 100, 50, 100),
            grab(feudal_click, 100, 100, 50, 100),
        ];
        let sup = feudal_sup(feudal_click);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Effective run: [click-90s, click-60s] = 30s — exactly at FLOATING_MIN_DURATION_MS
        assert_eq!(summary.get("Dark Age").unwrap().food, 30.0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end_ms, feudal_click - 60_000);
    }

    /// Gold is NOT suppressed before a feudal click — it should still be flagged.
    #[test]
    fn test_suppression_gold_not_suppressed_before_feudal() {
        let feudal_click = 500_000u64;
        let rows = vec![
            grab(feudal_click - 50_000, 100, 100, 400, 100),
            grab(feudal_click, 100, 100, 50, 100),
        ];
        let sup = feudal_sup(feudal_click);
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Gold threshold in Dark Age is 100; 400 > 100 for 50s → flagged
        assert_eq!(summary.get("Dark Age").unwrap().gold, 50.0);
    }

    /// Wood is never suppressed, even before a castle click.
    #[test]
    fn test_suppression_wood_never_suppressed() {
        let castle_click = 700_000u64;
        let rows = vec![
            grab(castle_click - 50_000, 100, 900, 50, 100),
            grab(castle_click, 100, 100, 50, 100),
        ];
        let sup = castle_age_sup(castle_click);
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Feudal wood threshold is 500; 900 > 500 for 50s → flagged
        assert_eq!(summary.get("Dark Age").unwrap().wood, 50.0);
    }

    /// Both food and gold are suppressed before a castle click.
    #[test]
    fn test_suppression_food_and_gold_before_castle() {
        let castle_click = 700_000u64;
        // Both runs entirely inside the window
        let rows = vec![
            grab(castle_click - 40_000, 900, 100, 600, 100),
            grab(castle_click, 100, 100, 50, 100),
        ];
        let sup = castle_age_sup(castle_click);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        let default = FloatingSeconds::default();
        let bucket = summary.get("Dark Age").unwrap_or(&default);
        assert_eq!(bucket.food, 0.0, "food should be suppressed");
        assert_eq!(bucket.gold, 0.0, "gold should be suppressed");
        assert!(segments.is_empty());
    }

    /// Stone run entirely inside a 60s castle-build window → discarded.
    #[test]
    fn test_suppression_stone_before_castle_fully_inside_window() {
        let build_ms = 800_000u64;
        let rows = vec![
            grab(build_ms - 40_000, 100, 100, 50, 600),
            grab(build_ms, 100, 100, 50, 100),
        ];
        let sup = castle_build_sup(build_ms);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Stone threshold in Dark Age is 200; 600 > 200 for 40s — but fully suppressed
        let default = FloatingSeconds::default();
        let bucket = summary.get("Dark Age").unwrap_or(&default);
        assert_eq!(bucket.stone, 0.0);
        assert!(segments.is_empty());
    }

    /// Stone run partially overlapping the castle-build window — pre-window portion counts.
    #[test]
    fn test_suppression_stone_before_castle_partial_overlap() {
        let build_ms = 800_000u64;
        // Run starts 90s before build: 30s before window, 60s inside window
        let rows = vec![
            grab(build_ms - 90_000, 100, 100, 50, 600),
            grab(build_ms, 100, 100, 50, 100),
        ];
        let sup = castle_build_sup(build_ms);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Effective run: [build-90s, build-60s] = 30s — exactly at FLOATING_MIN_DURATION_MS
        assert_eq!(summary.get("Dark Age").unwrap().stone, 30.0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end_ms, build_ms - 60_000);
    }

    /// Food and gold are NOT suppressed before a castle build — only stone is.
    #[test]
    fn test_suppression_castle_build_only_suppresses_stone() {
        let build_ms = 800_000u64;
        let rows = vec![
            grab(build_ms - 40_000, 900, 100, 600, 600),
            grab(build_ms, 100, 100, 50, 100),
        ];
        let sup = castle_build_sup(build_ms);
        let (summary, _) = calculate_floating_seconds(rows.into_iter(), &sup);
        let bucket = summary.get("Dark Age").unwrap();
        // Food (Dark threshold 200): 900 for 40s → flagged
        assert_eq!(bucket.food, 40.0);
        // Gold (Dark threshold 100): 600 for 40s → flagged
        assert_eq!(bucket.gold, 40.0);
        // Stone: suppressed
        assert_eq!(bucket.stone, 0.0);
    }

    /// A long food run before a feudal click: the early portion (> 30s) before
    /// the window still counts; only the last 60s are trimmed.
    #[test]
    fn test_suppression_early_float_still_counted() {
        let feudal_click = 500_000u64;
        // Float starts 120s before click: 60s outside window, 60s inside
        let rows = vec![
            grab(feudal_click - 120_000, 600, 100, 50, 100),
            grab(feudal_click, 100, 100, 50, 100),
        ];
        let sup = feudal_sup(feudal_click);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);
        // Effective: [click-120s, click-60s] = 60s
        assert_eq!(summary.get("Dark Age").unwrap().food, 60.0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_ms, feudal_click - 120_000);
        assert_eq!(segments[0].end_ms, feudal_click - 60_000);
    }

    /// A player floats 2000f for 2 minutes, clicks Feudal, then floats 1500f for another minute.
    /// Should result in 60s Dark Age float (pre-window) and 60s Feudal Age float.
    #[test]
    fn test_suppression_food_float_before_and_after_age_up() {
        let feudal_click = 120_000u64;
        let rows = vec![
            grab(0, 2000, 100, 50, 100),
            age_event(feudal_click, "Research Feudal Age"),
            grab(feudal_click, 1500, 100, 50, 100),
            grab(feudal_click + 60_000, 1500, 100, 50, 100),
        ];
        let sup = feudal_sup(feudal_click);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);

        assert_eq!(summary.get("Dark Age").unwrap().food, 60.0);
        assert_eq!(summary.get("Feudal Age").unwrap().food, 60.0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 60_000);
        assert_eq!(segments[0].age, "Dark Age");
        assert_eq!(segments[1].start_ms, 120_000);
        assert_eq!(segments[1].end_ms, 180_000);
        assert_eq!(segments[1].age, "Feudal Age");
    }

    /// A player floats 2000 stone for 2 minutes, builds a Castle, then floats 1350 stone for another minute.
    /// Should result in two 60s runs (pre-window and post-build).
    #[test]
    fn test_suppression_stone_float_before_and_after_castle() {
        let build_ms = 120_000u64;
        let rows = vec![
            grab(0, 100, 100, 50, 2000),
            age_event(build_ms, "Build Castle"),
            grab(build_ms, 100, 100, 50, 1350),
            grab(build_ms + 60_000, 100, 100, 50, 1350),
        ];
        let sup = castle_build_sup(build_ms);
        let (summary, segments) = calculate_floating_seconds(rows.into_iter(), &sup);

        assert_eq!(summary.get("Dark Age").unwrap().stone, 120.0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 60_000);
        assert_eq!(segments[1].start_ms, 120_000);
        assert_eq!(segments[1].end_ms, 180_000);
    }
}
