use crate::analysis::{floating::FloatingReport, housing::HousingReport, idle::IdleReport};
use crate::constants::{
    GRADE_GOLD_THRESHOLD_PCT, GRADE_SILVER_THRESHOLD_PCT, OVERALL_GOLD_MIN_STARS,
    OVERALL_SILVER_MIN_STARS,
};
use crate::replay::MatchMetadata;
use crate::types::{HousingStatus, Observation};
use minijinja::{context, Environment};
#[cfg(debug_assertions)]
use std::fs;

pub fn load_template() -> String {
    #[cfg(not(debug_assertions))]
    {
        include_str!("../templates/report.html").to_string()
    }

    #[cfg(debug_assertions)]
    {
        fs::read_to_string("templates/report.html").expect(
            "Failed to read templates/report.html. Ensure you are running from the project root.",
        )
    }
}

pub fn load_chart_js() -> String {
    #[cfg(not(debug_assertions))]
    {
        include_str!("../templates/js/chart.umd.min.js").to_string()
    }

    #[cfg(debug_assertions)]
    {
        fs::read_to_string("templates/js/chart.umd.min.js").expect(
            "Failed to read templates/js/chart.umd.min.js. Ensure you are running from the project root.",
        )
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ChartData {
    pub time_labels_json: String,
    pub pop_vils_json: String,
    pub idle_vils_json: String,
    pub housed_flags_json: String,
    pub age_ups_json: String, // Array of {label: string, age: string}
    pub floating_food_json: String,
    pub floating_wood_json: String,
    pub floating_gold_json: String,
    pub floating_stone_json: String,
    pub resource_food_json: String,
    pub resource_wood_json: String,
    pub resource_gold_json: String,
    pub resource_stone_json: String,
}

pub fn extract_chart_data(rows: &[Observation], floating_report: &FloatingReport) -> ChartData {
    let mut time_labels = Vec::new();
    let mut pop_vils = Vec::new();
    let mut idle_vils = Vec::new();
    let mut housed_flags = Vec::new();
    let mut floating_food = Vec::new();
    let mut floating_wood = Vec::new();
    let mut floating_gold = Vec::new();
    let mut floating_stone = Vec::new();
    let mut res_food = Vec::new();
    let mut res_wood = Vec::new();
    let mut res_gold = Vec::new();
    let mut res_stone = Vec::new();
    let mut age_ups = Vec::new();
    // Earlier clicks for the same age were cancelled; mark only the one that went through.
    let last_clicks = crate::analysis::last_age_up_clicks(rows);

    for row in rows {
        if let Observation::Screen {
            in_game_ms,
            reading,
        } = row
        {
            let label = crate::analysis::format_time(*in_game_ms);
            // simplify label by removing ms
            let label_no_ms = label.split('.').next().unwrap_or(&label).to_string();
            time_labels.push(label_no_ms);
            pop_vils.push(reading.pop_vils);
            idle_vils.push(reading.idle_vils);
            housed_flags.push(reading.housing == Some(HousingStatus::Housed));

            let ms = *in_game_ms;
            floating_food.push(
                floating_report
                    .segments
                    .iter()
                    .any(|s| s.resource == "Food" && ms >= s.start_ms && ms <= s.end_ms),
            );
            floating_wood.push(
                floating_report
                    .segments
                    .iter()
                    .any(|s| s.resource == "Wood" && ms >= s.start_ms && ms <= s.end_ms),
            );
            floating_gold.push(
                floating_report
                    .segments
                    .iter()
                    .any(|s| s.resource == "Gold" && ms >= s.start_ms && ms <= s.end_ms),
            );
            floating_stone.push(
                floating_report
                    .segments
                    .iter()
                    .any(|s| s.resource == "Stone" && ms >= s.start_ms && ms <= s.end_ms),
            );

            res_food.push(reading.food);
            res_wood.push(reading.wood);
            res_gold.push(reading.gold);
            res_stone.push(reading.stone);
        } else if let Some(age) = crate::analysis::effective_age_up(row, &last_clicks) {
            let label = crate::analysis::format_time(row.in_game_ms());
            let label_no_ms = label.split('.').next().unwrap_or(&label).to_string();
            age_ups.push(serde_json::json!({
                "label": label_no_ms,
                "age": format!("{} queued", age.name())
            }));
        }
    }

    ChartData {
        time_labels_json: serde_json::to_string(&time_labels).unwrap_or_else(|_| "[]".to_string()),
        pop_vils_json: serde_json::to_string(&pop_vils).unwrap_or_else(|_| "[]".to_string()),
        idle_vils_json: serde_json::to_string(&idle_vils).unwrap_or_else(|_| "[]".to_string()),
        housed_flags_json: serde_json::to_string(&housed_flags)
            .unwrap_or_else(|_| "[]".to_string()),
        age_ups_json: serde_json::to_string(&age_ups).unwrap_or_else(|_| "[]".to_string()),
        floating_food_json: serde_json::to_string(&floating_food)
            .unwrap_or_else(|_| "[]".to_string()),
        floating_wood_json: serde_json::to_string(&floating_wood)
            .unwrap_or_else(|_| "[]".to_string()),
        floating_gold_json: serde_json::to_string(&floating_gold)
            .unwrap_or_else(|_| "[]".to_string()),
        floating_stone_json: serde_json::to_string(&floating_stone)
            .unwrap_or_else(|_| "[]".to_string()),
        resource_food_json: serde_json::to_string(&res_food).unwrap_or_else(|_| "[]".to_string()),
        resource_wood_json: serde_json::to_string(&res_wood).unwrap_or_else(|_| "[]".to_string()),
        resource_gold_json: serde_json::to_string(&res_gold).unwrap_or_else(|_| "[]".to_string()),
        resource_stone_json: serde_json::to_string(&res_stone).unwrap_or_else(|_| "[]".to_string()),
    }
}

#[derive(Debug, serde::Serialize)]
pub struct SquireGrades {
    pub has_replay: bool,
    pub idle_pct: f64,
    pub idle_stars: u32,
    pub idle_grade: String,
    pub idle_tier: String,
    pub housed_pct: f64,
    pub housed_stars: u32,
    pub housed_grade: String,
    pub housed_tier: String,
    pub overall_grade: String,
    pub overall_tier: String,
}

pub fn calculate_squire_grades(
    has_replay: bool,
    idle_stats: &IdleReport,
    housing_stats: &HousingReport,
    duration_sec: f64,
) -> SquireGrades {
    if !has_replay || duration_sec <= 0.0 {
        return SquireGrades {
            has_replay: false,
            idle_pct: 0.0,
            idle_stars: 0,
            idle_grade: "—".to_string(),
            idle_tier: "none".to_string(),
            housed_pct: 0.0,
            housed_stars: 0,
            housed_grade: "—".to_string(),
            housed_tier: "none".to_string(),
            overall_grade: "—".to_string(),
            overall_tier: "none".to_string(),
        };
    }

    // 1. Idle Stats
    // Idle percentage: VS Lost / VS Total in percent
    let idle_pct = if idle_stats.total_vs_overall > 0.0 {
        (idle_stats.total_vs_lost / idle_stats.total_vs_overall * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };

    // Thresholds: Gold (3 stars), Silver (2 stars), else Bronze (1 star)
    let (idle_stars, idle_grade, idle_tier) = if idle_pct < GRADE_GOLD_THRESHOLD_PCT {
        (3, "★★★".to_string(), "gold".to_string())
    } else if idle_pct < GRADE_SILVER_THRESHOLD_PCT {
        (2, "★★".to_string(), "silver".to_string())
    } else {
        (1, "★".to_string(), "bronze".to_string())
    };

    // 2. Housed Stats
    // Housed percentage: Time Housed / Recorded match duration in percent
    let housed_pct = if housing_stats.recorded_duration_sec > 0.0 {
        ((housing_stats.total_housed_sec / housing_stats.recorded_duration_sec * 100.0) * 10.0)
            .round()
            / 10.0
    } else {
        0.0
    };

    // Thresholds: Gold (3 stars), Silver (2 stars), else Bronze (1 star)
    let (housed_stars, housed_grade, housed_tier) = if housed_pct < GRADE_GOLD_THRESHOLD_PCT {
        (3, "★★★".to_string(), "gold".to_string())
    } else if housed_pct < GRADE_SILVER_THRESHOLD_PCT {
        (2, "★★".to_string(), "silver".to_string())
    } else {
        (1, "★".to_string(), "bronze".to_string())
    };

    // 3. Overall Grade
    let total_stars = idle_stars + housed_stars;
    let (overall_grade, overall_tier) = if total_stars >= OVERALL_GOLD_MIN_STARS {
        ("★★★".to_string(), "gold".to_string())
    } else if total_stars >= OVERALL_SILVER_MIN_STARS {
        ("★★".to_string(), "silver".to_string())
    } else {
        ("★".to_string(), "bronze".to_string())
    };

    SquireGrades {
        has_replay: true,
        idle_pct,
        idle_stars,
        idle_grade,
        idle_tier,
        housed_pct,
        housed_stars,
        housed_grade,
        housed_tier,
        overall_grade,
        overall_tier,
    }
}

/// Builds the report filename: yyyymmdd-hhmmss-<player name>-<player civilization>.html
pub fn report_filename(metadata: &MatchMetadata) -> String {
    let date_part = metadata
        .start_time
        .replace(['-', ':'], "")
        .replace(' ', "-");
    format!(
        "{}-{}-{}.html",
        date_part,
        sanitize_filename_part(&metadata.rec_player_name),
        sanitize_filename_part(&metadata.rec_player_civ)
    )
}

/// Replaces characters that are invalid in Windows filenames (and spaces) with '_'.
fn sanitize_filename_part(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | ' ' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

pub fn generate_report(
    metadata: &MatchMetadata,
    idle_stats: &IdleReport,
    housing_stats: &HousingReport,
    floating_stats: &FloatingReport,
    chart_data: &ChartData,
) -> Result<String, minijinja::Error> {
    let template_str = load_template();

    // The .html extension enables minijinja's HTML auto-escaping; player and map
    // names are controlled by other players and must never be rendered raw.
    let mut env = Environment::new();
    env.add_template("report.html", &template_str)?;
    let tmpl = env.get_template("report.html")?;

    let chart_js_lib = load_chart_js();

    let grades = calculate_squire_grades(
        metadata.has_replay,
        idle_stats,
        housing_stats,
        metadata.duration_sec,
    );

    tmpl.render(context! {
        meta => metadata,
        idle => idle_stats,
        housing => housing_stats,
        floating => floating_stats,
        chart => chart_data,
        chart_js_lib => chart_js_lib,
        grades => grades,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::PlayerInfo;
    use std::collections::HashMap;

    fn sample_inputs() -> (IdleReport, HousingReport, FloatingReport, MatchMetadata) {
        let mut age_vs_lost = HashMap::new();
        age_vs_lost.insert("Dark Age".to_string(), 10.0);

        let idle = IdleReport {
            segments: vec![],
            age_vs_lost,
            total_vs_lost: 10.0,
            total_idle_duration_sec: 0.0,
            total_vs_overall: 100.0,
        };

        let mut metrics = HashMap::new();
        let mut dark_metrics = HashMap::new();
        dark_metrics.insert("housed".to_string(), 5.0);
        metrics.insert("Dark Age".to_string(), dark_metrics);

        let housing = HousingReport {
            segments: vec![],
            metrics,
            total_housed_sec: 5.0,
            total_queued_sec: 0.0,
            recorded_duration_sec: 2100.0,
        };
        use crate::analysis::floating::FloatingSeconds;
        let mut summary = HashMap::new();
        summary.insert("Dark Age".to_string(), FloatingSeconds::default());

        let floating = FloatingReport {
            summary,
            segments: vec![],
            total: FloatingSeconds::default(),
        };

        let metadata = MatchMetadata {
            start_time: "2026-05-06 12:00:00".to_string(),
            duration_formatted: "35:00.000".to_string(),
            duration_sec: 2100.0,
            players: vec![
                PlayerInfo {
                    name: "Player 1".to_string(),
                    civ: "Franks".to_string(),
                    is_winner: false,
                },
                PlayerInfo {
                    name: "Player 2".to_string(),
                    civ: "Britons".to_string(),
                    is_winner: false,
                },
            ],
            rec_player_name: "Player 1".to_string(),
            rec_player_civ: "Franks".to_string(),
            app_version: "0.1.0".to_string(),
            has_replay: true,
            map_name: "Arabia".to_string(),
        };

        (idle, housing, floating, metadata)
    }

    #[test]
    fn test_chart_marks_only_the_age_up_click_that_went_through() {
        let (_, _, floating, _) = sample_inputs();
        let click = |ms: u64, age: crate::types::Age| {
            crate::types::Observation::Event(crate::replay::ReplayEvent::TechResearch {
                timestamp_ms: ms as u32,
                player_id: 1,
                tech_id: age.tech_id().unwrap(),
                tech_type: age.name().to_string(),
                cost: crate::replay::ResourceCost {
                    food: 500,
                    wood: 0,
                    gold: 0,
                    stone: 0,
                },
                building_id: 1,
                building_type: "Town Center".to_string(),
            })
        };
        let rows = vec![
            click(400_000, crate::types::Age::Feudal), // cancelled
            click(470_000, crate::types::Age::Feudal),
            click(900_000, crate::types::Age::Castle),
        ];
        let chart = extract_chart_data(&rows, &floating);
        assert_eq!(
            chart.age_ups_json,
            r#"[{"age":"Feudal Age queued","label":"07:50"},{"age":"Castle Age queued","label":"15:00"}]"#
        );
    }

    #[test]
    fn test_template_loads_and_renders() {
        let (idle, housing, floating, metadata) = sample_inputs();
        let chart_data = extract_chart_data(&[], &floating);

        let html = generate_report(&metadata, &idle, &housing, &floating, &chart_data)
            .expect("Template should render successfully");
        assert!(
            html.contains("Franks vs Britons"),
            "HTML should contain the player civilizations in the title"
        );
        assert!(
            html.contains("AoE2 Squire"),
            "HTML should contain the app name in the title"
        );
        assert!(
            html.contains("Idle Villagers"),
            "HTML should contain the Idle Villagers legend entry"
        );
        assert!(
            html.contains("matchChart"),
            "HTML should contain the chart canvas element"
        );
    }

    #[test]
    fn test_player_controlled_strings_are_escaped() {
        let (idle, housing, floating, mut metadata) = sample_inputs();
        metadata.players[1].name = "<img src=x onerror=alert(1)>".to_string();
        metadata.map_name = "<script>alert(2)</script>".to_string();
        let chart_data = extract_chart_data(&[], &floating);

        let html = generate_report(&metadata, &idle, &housing, &floating, &chart_data)
            .expect("Template should render successfully");
        assert!(
            !html.contains("<img src=x"),
            "player name must not be rendered raw"
        );
        assert!(
            html.contains("&lt;img src=x"),
            "player name should be HTML-escaped"
        );
        assert!(
            !html.contains("<script>alert(2)"),
            "map name must not be rendered raw"
        );
        // The chart data is injected with `| safe` and must not be escaped.
        assert!(html.contains("const timeLabels = [];"));
    }

    #[test]
    fn test_report_filename_is_valid_on_windows() {
        let (_, _, _, mut metadata) = sample_inputs();
        assert_eq!(
            report_filename(&metadata),
            "20260506-120000-Player_1-Franks.html"
        );

        metadata.rec_player_name = "a<b>c:d\"e/f\\g|h?i*j\tk".to_string();
        assert_eq!(
            report_filename(&metadata),
            "20260506-120000-a_b_c_d_e_f_g_h_i_j_k-Franks.html"
        );
    }

    #[test]
    fn test_calculate_squire_grades() {
        // Helper to construct reports with simplified values
        let make_reports = |vs_lost: f64, vs_overall: f64, housed_sec: f64, recorded_sec: f64| {
            (
                IdleReport {
                    segments: vec![],
                    age_vs_lost: HashMap::new(),
                    total_vs_lost: vs_lost,
                    total_idle_duration_sec: 0.0,
                    total_vs_overall: vs_overall,
                },
                HousingReport {
                    segments: vec![],
                    metrics: HashMap::new(),
                    total_housed_sec: housed_sec,
                    total_queued_sec: 0.0,
                    recorded_duration_sec: recorded_sec,
                },
            )
        };

        // 1. Gold + Gold = 6 stars -> Gold overall (since 6 >= OVERALL_GOLD_MIN_STARS)
        // Gold target is < 2.0%. 1.5% idle, 1.0% housed.
        let (idle, housing) = make_reports(15.0, 1000.0, 10.0, 1000.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "gold");
        assert_eq!(grades.housed_tier, "gold");
        assert_eq!(grades.overall_tier, "gold");

        // 2. Gold + Silver = 5 stars -> Silver overall (since 5 < OVERALL_GOLD_MIN_STARS)
        // Silver target is < 4.0%. 1.5% idle (gold), 3.0% housed (silver).
        let (idle, housing) = make_reports(15.0, 1000.0, 30.0, 1000.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "gold");
        assert_eq!(grades.housed_tier, "silver");
        assert_eq!(grades.overall_tier, "silver");

        // 3. Silver + Silver = 4 stars -> Silver overall (since 4 >= OVERALL_SILVER_MIN_STARS)
        // 3.0% idle (silver), 3.0% housed (silver).
        let (idle, housing) = make_reports(30.0, 1000.0, 30.0, 1000.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "silver");
        assert_eq!(grades.housed_tier, "silver");
        assert_eq!(grades.overall_tier, "silver");

        // 4. Gold + Bronze = 4 stars -> Silver overall (since 4 >= OVERALL_SILVER_MIN_STARS)
        // 1.5% idle (gold), 5.0% housed (bronze).
        let (idle, housing) = make_reports(15.0, 1000.0, 50.0, 1000.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "gold");
        assert_eq!(grades.housed_tier, "bronze");
        assert_eq!(grades.overall_tier, "silver");

        // 5. Bronze + Bronze = 2 stars -> Bronze overall (since 2 < OVERALL_SILVER_MIN_STARS)
        // 5.0% idle, 5.0% housed.
        let (idle, housing) = make_reports(50.0, 1000.0, 50.0, 1000.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "bronze");
        assert_eq!(grades.housed_tier, "bronze");
        assert_eq!(grades.overall_tier, "bronze");

        // 6. No replay fallback -> tier is "none"
        let (idle, housing) = make_reports(1.0, 1000.0, 1.0, 1000.0);
        let grades = calculate_squire_grades(false, &idle, &housing, 1000.0);
        assert_eq!(grades.idle_tier, "none");
        assert_eq!(grades.housed_tier, "none");
        assert_eq!(grades.overall_tier, "none");

        // 7. Captured duration shorter than full game duration
        // Total match: 2000s, Captured/recorded: 500s, Housed: 10s.
        // If we incorrectly divide by 2000s, housed_pct = 0.5% (gold).
        // If we correctly divide by 500s, housed_pct = 2.0% (silver).
        let (idle, housing) = make_reports(15.0, 1000.0, 10.0, 500.0);
        let grades = calculate_squire_grades(true, &idle, &housing, 2000.0);
        assert_eq!(grades.housed_tier, "silver");
    }
}
