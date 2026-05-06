use crate::analysis::{floating::FloatingReport, housing::HousingReport, idle::IdleReport};
use crate::replay::MatchMetadata;
use minijinja::{context, Environment};
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

pub fn generate_report(
    metadata: &MatchMetadata,
    idle_stats: &IdleReport,
    housing_stats: &HousingReport,
    floating_stats: &FloatingReport,
) -> Result<String, minijinja::Error> {
    let template_str = load_template();

    let mut env = Environment::new();
    env.add_template("report", &template_str)?;
    let tmpl = env.get_template("report")?;

    tmpl.render(context! {
        meta => metadata,
        idle => idle_stats,
        housing => housing_stats,
        floating => floating_stats,
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
        };

        let html = generate_report(&metadata, &idle, &housing, &floating)
            .expect("Template should render successfully");
        assert!(
            html.contains("RTS Match Analysis Report"),
            "HTML should contain the title"
        );
        assert!(
            html.contains("Idle Villagers"),
            "HTML should contain Idle section"
        );
        assert!(
            html.contains("Housing Efficiency"),
            "HTML should contain Housing section"
        );
        assert!(
            html.contains("Floating Resources"),
            "HTML should contain Floating section"
        );
    }
}
