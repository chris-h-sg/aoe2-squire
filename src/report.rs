use crate::analysis::{floating::FloatingReport, housing::HousingReport, idle::IdleReport};
use crate::replay::MatchMetadata;
use crate::types::MergedRow;
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

pub fn extract_chart_data(rows: &[MergedRow], floating_report: &FloatingReport) -> ChartData {
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

    for row in rows {
        if row.observation_type == "ScreenGrab" {
            let label = crate::analysis::format_time(row.in_game_ms);
            // simplify label by removing ms
            let label_no_ms = label.split('.').next().unwrap_or(&label).to_string();
            time_labels.push(label_no_ms);
            pop_vils.push(row.pop_vils.parse::<u32>().ok());
            idle_vils.push(row.idle_vils.parse::<u32>().ok());
            housed_flags.push(row.housing == "housed");

            let ms = row.in_game_ms;
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

            res_food.push(row.food.parse::<u32>().ok());
            res_wood.push(row.wood.parse::<u32>().ok());
            res_gold.push(row.gold.parse::<u32>().ok());
            res_stone.push(row.stone.parse::<u32>().ok());
        } else if row.observation_type == "RecEvent"
            && row.event_desc.starts_with("Research ")
            && row.event_desc.ends_with(" Age")
        {
            let label = crate::analysis::format_time(row.in_game_ms);
            let label_no_ms = label.split('.').next().unwrap_or(&label).to_string();
            age_ups.push(serde_json::json!({
                "label": label_no_ms,
                "age": format!("{} queued", row.event_desc.replace("Research ", ""))
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

pub fn generate_report(
    metadata: &MatchMetadata,
    idle_stats: &IdleReport,
    housing_stats: &HousingReport,
    floating_stats: &FloatingReport,
    chart_data: &ChartData,
) -> Result<String, minijinja::Error> {
    let template_str = load_template();

    let mut env = Environment::new();
    env.add_template("report", &template_str)?;
    let tmpl = env.get_template("report")?;

    let chart_js_lib = load_chart_js();

    tmpl.render(context! {
        meta => metadata,
        idle => idle_stats,
        housing => housing_stats,
        floating => floating_stats,
        chart => chart_data,
        chart_js_lib => chart_js_lib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::PlayerInfo;
    use std::collections::HashMap;

    #[test]
    fn test_template_loads_and_renders() {
        let mut age_vs_lost = HashMap::new();
        age_vs_lost.insert("Dark Age".to_string(), 10.0);

        let idle = IdleReport {
            segments: vec![],
            age_vs_lost,
            total_vs_lost: 10.0,
            total_idle_duration_sec: 0.0,
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
                },
                PlayerInfo {
                    name: "Player 2".to_string(),
                    civ: "Britons".to_string(),
                },
            ],
            rec_player_name: "Player 1".to_string(),
            rec_player_civ: "Franks".to_string(),
            app_version: "0.1.0".to_string(),
            has_replay: true,
        };

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
}
