use crate::replay::{ReplayEvent, ResourceCost};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct UiElement {
    pub x_px: f64,
    pub y_px: f64,
    pub w_px: f64,
    pub h_px: f64,
    // split_pct exists in the JSON but is unused by the pipeline
    #[allow(dead_code)]
    pub split_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UiMap {
    pub baseline_margin: f64,
    pub elements: HashMap<String, UiElement>,
}

/// char -> flat f32 array of size CANVAS_SIZE*CANVAS_SIZE, values in [0.0, 1.0]
pub type Templates = HashMap<char, Vec<f32>>;

/// category (e.g. "wood") -> sub_key (e.g. "total") -> digit string (e.g. "423")
pub type Results = HashMap<String, HashMap<String, String>>;

/// State of the population display: normal, housed (overlay) or queue-blocked (yellow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HousingStatus {
    Normal,
    Housed,
    Queued,
}

impl HousingStatus {
    /// The value used in the telemetry CSV and the reports.
    pub fn as_str(self) -> &'static str {
        match self {
            HousingStatus::Normal => "normal",
            HousingStatus::Housed => "housed",
            HousingStatus::Queued => "queued",
        }
    }

    /// Parses a telemetry CSV value; anything else (e.g. an empty field) is None.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "normal" => Some(HousingStatus::Normal),
            "housed" => Some(HousingStatus::Housed),
            "queued" => Some(HousingStatus::Queued),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Age {
    Dark,
    Feudal,
    Castle,
    Imperial,
}

impl Age {
    /// The name used in reports and in `constants::GAME_AGES`.
    pub fn name(self) -> &'static str {
        match self {
            Age::Dark => "Dark Age",
            Age::Feudal => "Feudal Age",
            Age::Castle => "Castle Age",
            Age::Imperial => "Imperial Age",
        }
    }

    /// Position in `GAME_AGES` / `FLOATING_THRESHOLDS`.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The age a technology advances to, by its name in data/techs.csv.
    pub fn from_tech(tech: &str) -> Option<Self> {
        match tech {
            "Feudal Age" => Some(Age::Feudal),
            "Castle Age" => Some(Age::Castle),
            "Imperial Age" => Some(Age::Imperial),
            _ => None,
        }
    }
}

/// One row of the telemetry CSV: the resource panel as read at `timestamp_ms` (real-world).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct CsvRow {
    pub timestamp_ms: u64,
    pub food: Option<u32>,
    pub wood: Option<u32>,
    pub gold: Option<u32>,
    pub stone: Option<u32>,
    pub pop_curr: Option<u32>,
    pub pop_max: Option<u32>,
    pub pop_vils: Option<u32>,
    pub idle_vils: Option<u32>,
    pub housing: Option<HousingStatus>,
}

/// One entry of the merged in-game timeline built by the mesher.
#[derive(Debug, Clone)]
pub enum Observation {
    /// A telemetry row, placed on the in-game clock by the sync calibration.
    Screen { in_game_ms: u64, reading: CsvRow },
    /// A replay command of the recorded player.
    Event(ReplayEvent),
}

impl Observation {
    pub fn in_game_ms(&self) -> u64 {
        match self {
            Observation::Screen { in_game_ms, .. } => *in_game_ms,
            Observation::Event(event) => event.timestamp_ms() as u64,
        }
    }

    pub fn reading(&self) -> Option<&CsvRow> {
        match self {
            Observation::Screen { reading, .. } => Some(reading),
            Observation::Event(_) => None,
        }
    }

    pub fn event(&self) -> Option<&ReplayEvent> {
        match self {
            Observation::Event(event) => Some(event),
            Observation::Screen { .. } => None,
        }
    }

    /// Rebuilds an observation from a row of `output/merged_observations.csv`. Replay
    /// details the file does not hold (player, object and building type ids, positions,
    /// queue position) are zero. None for an unknown row.
    pub fn from_merged_row(row: &MergedRow) -> Option<Self> {
        let num = |s: &str| s.parse::<u32>().ok();
        match row.observation_type.as_str() {
            "ScreenGrab" => Some(Observation::Screen {
                in_game_ms: row.in_game_ms,
                reading: CsvRow {
                    timestamp_ms: row.rw_timestamp_ms.parse().unwrap_or(0),
                    food: num(&row.food),
                    wood: num(&row.wood),
                    gold: num(&row.gold),
                    stone: num(&row.stone),
                    pop_curr: num(&row.pop_curr),
                    pop_max: num(&row.pop_max),
                    pop_vils: num(&row.pop_vils),
                    idle_vils: num(&row.idle_vils),
                    housing: HousingStatus::parse(&row.housing),
                },
            }),
            "RecEvent" => {
                let timestamp_ms = row.in_game_ms as u32;
                let player_id = 0;
                let spent = |s: &str| s.trim_start_matches('-').parse::<u32>().unwrap_or(0);
                let cost = ResourceCost {
                    food: spent(&row.food),
                    wood: spent(&row.wood),
                    gold: spent(&row.gold),
                    stone: spent(&row.stone),
                };
                let building_ids: Vec<u32> = row
                    .building_ids
                    .split(',')
                    .filter_map(|id| id.parse().ok())
                    .collect();
                let building_types = || -> Vec<String> {
                    row.building_type
                        .split(',')
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .collect()
                };
                let desc = row.event_desc.as_str();
                let event = if let Some(tech) = desc.strip_prefix("Research ") {
                    ReplayEvent::TechResearch {
                        timestamp_ms,
                        player_id,
                        tech_type: tech.to_string(),
                        cost,
                        building_id: building_ids.first().copied().unwrap_or(0),
                        building_type: row.building_type.clone(),
                    }
                } else if let Some(unit) = desc.strip_prefix("Queue ") {
                    ReplayEvent::UnitQueued {
                        timestamp_ms,
                        player_id,
                        unit_type: unit.to_string(),
                        cost,
                        building_types: building_types(),
                        building_ids,
                    }
                } else if desc == "Unqueue" {
                    ReplayEvent::QueueCancellation {
                        timestamp_ms,
                        player_id,
                        building_types: building_types(),
                        building_ids,
                        queue_position: 0,
                    }
                } else if let Some(building) = desc.strip_prefix("Build ") {
                    ReplayEvent::BuildingConstruction {
                        timestamp_ms,
                        player_id,
                        building_type: building.to_string(),
                        building_type_id: 0,
                        x: 0.0,
                        y: 0.0,
                        cost,
                    }
                } else {
                    ReplayEvent::Deletion {
                        timestamp_ms,
                        player_id,
                        object_id: 0,
                        object_name: desc.strip_prefix("Delete ")?.to_string(),
                    }
                };
                Some(Observation::Event(event))
            }
            _ => None,
        }
    }
}

/// A row of `output/merged_observations.csv` (written by the `mesher` tool, read by the
/// single-analyzer tools); in memory the timeline is a `Vec<Observation>`.
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct MergedRow {
    pub in_game_ms: u64,
    pub observation_type: String,
    pub event_desc: String,
    pub food: String,
    pub wood: String,
    pub gold: String,
    pub stone: String,
    pub rw_timestamp_ms: String,
    pub building_type: String,
    pub building_ids: String,
    pub idle_vils: String,
    pub pop_curr: String,
    pub pop_max: String,
    pub pop_vils: String,
    pub housing: String,
}

impl From<&Observation> for MergedRow {
    fn from(obs: &Observation) -> Self {
        let opt = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
        match obs {
            Observation::Screen {
                in_game_ms,
                reading,
            } => MergedRow {
                in_game_ms: *in_game_ms,
                observation_type: "ScreenGrab".to_string(),
                food: opt(reading.food),
                wood: opt(reading.wood),
                gold: opt(reading.gold),
                stone: opt(reading.stone),
                rw_timestamp_ms: reading.timestamp_ms.to_string(),
                idle_vils: opt(reading.idle_vils),
                pop_curr: opt(reading.pop_curr),
                pop_max: opt(reading.pop_max),
                pop_vils: opt(reading.pop_vils),
                housing: reading
                    .housing
                    .map(|h| h.as_str().to_string())
                    .unwrap_or_default(),
                ..Default::default()
            },
            Observation::Event(event) => {
                let spent = |amount: fn(&ResourceCost) -> u32| {
                    event
                        .cost()
                        .map(amount)
                        .filter(|&a| a > 0)
                        .map(|a| format!("-{}", a))
                        .unwrap_or_default()
                };
                MergedRow {
                    in_game_ms: obs.in_game_ms(),
                    observation_type: "RecEvent".to_string(),
                    event_desc: event.description(),
                    food: spent(|c| c.food),
                    wood: spent(|c| c.wood),
                    gold: spent(|c| c.gold),
                    stone: spent(|c| c.stone),
                    building_type: event.building_type(),
                    building_ids: event
                        .building_ids()
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    ..Default::default()
                }
            }
        }
    }
}
