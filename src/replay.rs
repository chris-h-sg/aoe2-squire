use aoe2rec::Savegame;
use binrw::BinReaderExt;
use chrono::{Local, TimeZone};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug, Serialize, Clone)]
pub struct ResourceCost {
    pub food: u32,
    pub wood: u32,
    pub gold: u32,
    pub stone: u32,
}

#[derive(Debug, Serialize)]
pub enum ReplayEvent {
    TechResearch {
        timestamp_ms: u32,
        player_id: u8,
        tech_type: String,
        cost: ResourceCost,
        building_id: u32,
        building_type: String,
    },
    UnitQueued {
        timestamp_ms: u32,
        player_id: u8,
        unit_type: String,
        cost: ResourceCost,
        building_ids: Vec<u32>,
        building_types: Vec<String>,
    },
    QueueCancellation {
        timestamp_ms: u32,
        player_id: u8,
        building_ids: Vec<u32>,
        building_types: Vec<String>,
        queue_position: i32,
    },
    BuildingConstruction {
        timestamp_ms: u32,
        player_id: u8,
        building_type: String,
        building_type_id: u32,
        x: f32,
        y: f32,
        cost: ResourceCost,
    },
    Deletion {
        timestamp_ms: u32,
        player_id: u8,
        object_id: u32,
        object_name: String,
    },
}

pub fn format_time(ms: u32) -> String {
    let seconds = (ms as f32) / 1000.0;
    let minutes = (seconds / 60.0).floor() as u32;
    let remaining_seconds = seconds % 60.0;
    format!("{:02}:{:06.3}", minutes, remaining_seconds)
}

type ReferenceData = (
    HashMap<u32, String>,
    HashMap<u32, ResourceCost>,
    HashMap<u32, u32>,
);

fn parse_csv_map(
    content: &str,
    id_col: usize,
    name_col: usize,
    cost_start_col: Option<usize>,
    building_col: Option<usize>,
    num_costs: usize,
) -> Result<ReferenceData, Box<dyn std::error::Error>> {
    let mut name_map = HashMap::new();
    let mut cost_map = HashMap::new();
    let mut building_map = HashMap::new();

    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    for result in rdr.records() {
        let record = result?;
        if record.len() > id_col && !record[id_col].trim().is_empty() {
            if let Ok(id) = record[id_col].parse::<u32>() {
                if record.len() > name_col {
                    name_map.insert(id, record[name_col].to_string());
                }

                if let Some(start) = cost_start_col {
                    let food = record.get(start).and_then(|v| v.parse().ok()).unwrap_or(0);
                    let wood = if num_costs > 1 {
                        record
                            .get(start + 1)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let gold = if num_costs > 2 {
                        record
                            .get(start + 2)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let stone = if num_costs > 3 {
                        record
                            .get(start + 3)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };

                    cost_map.insert(
                        id,
                        ResourceCost {
                            food,
                            wood,
                            gold,
                            stone,
                        },
                    );
                }

                if let Some(col) = building_col {
                    if record.len() > col && !record[col].trim().is_empty() {
                        if let Ok(b_id) = record[col].parse::<u32>() {
                            building_map.insert(id, b_id);
                        }
                    }
                }
            }
        }
    }
    Ok((name_map, cost_map, building_map))
}

#[derive(Debug, Serialize, Clone)]
pub struct PlayerInfo {
    pub name: String,
    pub civ: String,
    pub is_winner: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct MatchMetadata {
    pub start_time: String,
    pub duration_formatted: String,
    pub duration_sec: f64,
    pub players: Vec<PlayerInfo>,
    pub rec_player_name: String,
    pub rec_player_civ: String,
    pub app_version: String,
    pub has_replay: bool,
}

pub struct ReplayData {
    pub rec_player: u32,
    pub player_names: HashMap<u8, String>,
    pub events: Vec<ReplayEvent>,
    pub metadata: MatchMetadata,
}

pub fn extract_events(replay_path: &Path) -> Result<ReplayData, Box<dyn std::error::Error>> {
    let file = File::open(replay_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    let mut cursor = std::io::Cursor::new(&buffer);
    let savegame: Savegame = cursor.read_le()?;

    println!("Replay parsed successfully!");
    println!("Build: {:?}", savegame.zheader.build);

    // Load reference data
    let units_csv = include_str!("../data/units.csv");
    let techs_csv = include_str!("../data/techs.csv");
    let buildings_csv = include_str!("../data/buildings.csv");
    let civs_csv = include_str!("../data/civilizations.csv");

    let (unit_map, unit_cost_map, unit_to_b_raw) =
        parse_csv_map(units_csv, 0, 1, Some(6), Some(7), 3)?;
    let (tech_map, tech_cost_map, tech_to_b_raw) =
        parse_csv_map(techs_csv, 0, 1, Some(2), Some(6), 4)?;
    let (building_map, building_cost_map, _) =
        parse_csv_map(buildings_csv, 0, 1, Some(4), None, 4)?;
    let (civ_map, _, _) = parse_csv_map(civs_csv, 0, 1, None, None, 0)?;

    // Map building IDs to names for easier lookup
    let unit_to_b_map: HashMap<u32, String> = unit_to_b_raw
        .into_iter()
        .filter_map(|(u_id, b_id)| building_map.get(&b_id).map(|name| (u_id, name.clone())))
        .collect();
    let tech_to_b_map: HashMap<u32, String> = tech_to_b_raw
        .into_iter()
        .filter_map(|(t_id, b_id)| building_map.get(&b_id).map(|name| (t_id, name.clone())))
        .collect();

    let mut events = Vec::new();
    let mut instance_map: HashMap<u32, String> = HashMap::new();
    let mut pending_builds: HashMap<(i32, i32), String> = HashMap::new();

    for obj in &savegame
        .zheader
        .initial
        .initial_tail
        .initial_object_instances
    {
        // Try to identify the object name from buildings or units
        let name = building_map
            .get(&(obj.object_type_id as u32))
            .or_else(|| unit_map.get(&(obj.object_type_id as u32)))
            .cloned()
            .unwrap_or_else(|| format!("Unknown ({})", obj.object_type_id));

        instance_map.insert(obj.object_id, name.clone());
    }

    let mut player_names = HashMap::new();
    let mut player_infos = Vec::new();

    for (i, player) in savegame.zheader.game_settings.players.iter().enumerate() {
        let name: String = player.name.clone().into();
        let ai_name: String = player.ai_name.clone().into();
        let display_name = if !name.trim().is_empty() {
            name
        } else if !ai_name.trim().is_empty() {
            ai_name
        } else {
            format!("Player {}", i + 1)
        };
        player_names.insert((i + 1) as u8, display_name.clone());

        let civ_name = civ_map
            .get(&player.civ_id)
            .cloned()
            .unwrap_or_else(|| format!("Unknown Civ {}", player.civ_id));

        if player.player_type == 2 || player.player_type == 4 {
            // Only valid human/AI players, 2=Human, 4=AI
            player_infos.push(PlayerInfo {
                name: display_name,
                civ: civ_name,
                is_winner: false,
            });
        } else if display_name != format!("Player {}", i + 1) && !display_name.trim().is_empty() {
            // fallback if player_type is different but it has a real name
            player_infos.push(PlayerInfo {
                name: display_name,
                civ: civ_name,
                is_winner: false,
            });
        }
    }

    // Ensure POV player is first in the list
    let pov_id = savegame.zheader.replay.rec_player as u8;
    if let Some(pov_name) = player_names.get(&pov_id) {
        if let Some(pos) = player_infos.iter().position(|p| &p.name == pov_name) {
            let pov_info = player_infos.remove(pos);
            player_infos.insert(0, pov_info);
        }
    }

    let mut current_ms = 0;
    let mut loser_id: Option<u8> = None;

    for op in savegame.operations {
        match op {
            aoe2rec::Operation::Sync { time_increment, .. } => {
                current_ms += time_increment;
            }
            aoe2rec::Operation::Action {
                action_data: Some(action),
                ..
            } => match action {
                aoe2rec::actions::ActionData::Research {
                    player_id,
                    technology_type,
                    building_id,
                    ..
                } => {
                    let tech_name = tech_map
                        .get(&(technology_type as u32))
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Tech");

                    if let Some(b_type) = tech_to_b_map.get(&(technology_type as u32)) {
                        instance_map.insert(building_id, b_type.clone());
                    }

                    let b_type_name = instance_map
                        .get(&building_id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown Building".to_string());

                    let cost = tech_cost_map
                        .get(&(technology_type as u32))
                        .cloned()
                        .unwrap_or(ResourceCost {
                            food: 0,
                            wood: 0,
                            gold: 0,
                            stone: 0,
                        });

                    events.push(ReplayEvent::TechResearch {
                        timestamp_ms: current_ms,
                        player_id,
                        tech_type: tech_name.to_string(),
                        cost,
                        building_id,
                        building_type: b_type_name,
                    });
                }
                aoe2rec::actions::ActionData::DeQueue {
                    player_id,
                    unit_id,
                    amount,
                    building_type,
                    building_ids,
                    ..
                } => {
                    let unit_id = unit_id as u32;
                    let b_type_name = building_map
                        .get(&(building_type as u32))
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Building")
                        .to_string();

                    for b_id in &building_ids {
                        instance_map.insert(*b_id, b_type_name.clone());
                    }

                    let base_cost = unit_cost_map
                        .get(&unit_id)
                        .cloned()
                        .unwrap_or(ResourceCost {
                            food: 0,
                            wood: 0,
                            gold: 0,
                            stone: 0,
                        });

                    let buildings_count = building_ids.len() as u32;
                    let total_cost = ResourceCost {
                        food: base_cost.food * buildings_count,
                        wood: base_cost.wood * buildings_count,
                        gold: base_cost.gold * buildings_count,
                        stone: base_cost.stone * buildings_count,
                    };

                    for _ in 0..amount {
                        let unit_name = unit_map
                            .get(&unit_id)
                            .map(|s| s.as_str())
                            .unwrap_or("Unknown Unit");

                        events.push(ReplayEvent::UnitQueued {
                            timestamp_ms: current_ms,
                            player_id,
                            unit_type: unit_name.to_string(),
                            cost: total_cost.clone(),
                            building_ids: building_ids.clone(),
                            building_types: vec![b_type_name.clone(); building_ids.len()],
                        });
                    }
                }
                aoe2rec::actions::ActionData::Order {
                    player_id,
                    order_type,
                    unknown4,
                    object_ids,
                    unknown5,
                    ..
                } => {
                    let unit_id = unknown4 as u32;
                    let building_ids = object_ids;

                    if let Some(b_type) = unit_to_b_map.get(&unit_id) {
                        for b_id in &building_ids {
                            instance_map.insert(*b_id, b_type.clone());
                        }
                    }

                    if order_type == aoe2rec::actions::OrderType::Unqueue {
                        let building_types: Vec<String> = building_ids
                            .iter()
                            .map(|id| {
                                instance_map
                                    .get(id)
                                    .cloned()
                                    .unwrap_or_else(|| "Unknown Building".to_string())
                            })
                            .collect();

                        events.push(ReplayEvent::QueueCancellation {
                            timestamp_ms: current_ms,
                            player_id,
                            building_ids,
                            building_types,
                            queue_position: unknown5 as i32,
                        });
                    }
                }
                aoe2rec::actions::ActionData::Queue {
                    player_id,
                    unit_type,
                    building_ids,
                    count,
                    ..
                } => {
                    let unit_id = unit_type as u32;
                    if let Some(b_type) = unit_to_b_map.get(&unit_id) {
                        for b_id in &building_ids {
                            instance_map.insert(*b_id, b_type.clone());
                        }
                    }

                    let b_type_name = if !building_ids.is_empty() {
                        instance_map
                            .get(&building_ids[0])
                            .cloned()
                            .unwrap_or_else(|| "Unknown Building".to_string())
                    } else {
                        "Unknown Building".to_string()
                    };

                    let unit_name = unit_map
                        .get(&unit_id)
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Unit");

                    let base_cost = unit_cost_map
                        .get(&unit_id)
                        .cloned()
                        .unwrap_or(ResourceCost {
                            food: 0,
                            wood: 0,
                            gold: 0,
                            stone: 0,
                        });

                    let buildings_count = building_ids.len() as u32;
                    let total_cost = ResourceCost {
                        food: base_cost.food * buildings_count,
                        wood: base_cost.wood * buildings_count,
                        gold: base_cost.gold * buildings_count,
                        stone: base_cost.stone * buildings_count,
                    };

                    for _ in 0..count {
                        events.push(ReplayEvent::UnitQueued {
                            timestamp_ms: current_ms,
                            player_id,
                            unit_type: unit_name.to_string(),
                            cost: total_cost.clone(),
                            building_ids: building_ids.clone(),
                            building_types: vec![b_type_name.clone(); building_ids.len()],
                        });
                    }
                }
                aoe2rec::actions::ActionData::Build {
                    player_id,
                    building_type_id,
                    x,
                    y,
                    ..
                } => {
                    let b_type_name = building_map
                        .get(&building_type_id)
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Building")
                        .to_string();

                    let cost =
                        building_cost_map
                            .get(&building_type_id)
                            .cloned()
                            .unwrap_or(ResourceCost {
                                food: 0,
                                wood: 0,
                                gold: 0,
                                stone: 0,
                            });

                    let pos_key = ((x * 10.0) as i32, (y * 10.0) as i32);
                    pending_builds.insert(pos_key, b_type_name.clone());

                    events.push(ReplayEvent::BuildingConstruction {
                        timestamp_ms: current_ms,
                        player_id,
                        building_type: b_type_name,
                        building_type_id,
                        x,
                        y,
                        cost,
                    });
                }
                aoe2rec::actions::ActionData::Interact {
                    target_id, x, y, ..
                } => {
                    let pos_key = ((x * 10.0) as i32, (y * 10.0) as i32);
                    if let Some(b_type_name) = pending_builds.get(&pos_key) {
                        if target_id > 0 && !instance_map.contains_key(&target_id) {
                            instance_map.insert(target_id, b_type_name.clone());
                        }
                    } else {
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                let neighbor_key = (pos_key.0 + dx, pos_key.1 + dy);
                                if let Some(b_type_name) = pending_builds.get(&neighbor_key) {
                                    if target_id > 0 && !instance_map.contains_key(&target_id) {
                                        instance_map.insert(target_id, b_type_name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                aoe2rec::actions::ActionData::Delete {
                    player_id,
                    object_id,
                    ..
                } => {
                    let name = instance_map
                        .get(&object_id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown".to_string());

                    events.push(ReplayEvent::Deletion {
                        timestamp_ms: current_ms,
                        player_id,
                        object_id,
                        object_name: name,
                    });
                }
                aoe2rec::actions::ActionData::Resign {
                    player_id,
                    ..
                } => {
                    loser_id = Some(player_id);
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some(l_id) = loser_id {
        if let Some(loser_name) = player_names.get(&l_id) {
            for p in &mut player_infos {
                if p.name != *loser_name {
                    p.is_winner = true;
                }
            }
        }
    }

    let datetime = Local
        .timestamp_opt(savegame.zheader.timestamp as i64, 0)
        .unwrap();
    let start_time = datetime.format("%Y-%m-%d %H:%M:%S").to_string();

    let (rec_player_name, rec_player_civ) = player_names
        .get(&(savegame.zheader.replay.rec_player as u8))
        .map(|name| {
            let civ = player_infos
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.civ.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            (name.clone(), civ)
        })
        .unwrap_or_else(|| ("Unknown".to_string(), "Unknown".to_string()));

    let metadata = MatchMetadata {
        start_time,
        duration_formatted: format_time(current_ms),
        duration_sec: current_ms as f64 / 1000.0,
        players: player_infos,
        rec_player_name,
        rec_player_civ,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        has_replay: true,
    };

    Ok(ReplayData {
        rec_player: savegame.zheader.replay.rec_player as u32,
        player_names,
        events,
        metadata,
    })
}

pub fn print_events(replay_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let data = extract_events(replay_path)?;
    let rec_player = data.rec_player;
    let player_names = data.player_names;
    let events = data.events;
    println!("POV Player ID: {}", rec_player);

    println!(
        "\n{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}",
        "Time",
        "Player",
        "Event",
        "Item",
        "Building Type(s)",
        "Building ID(s)",
        "Food",
        "Wood",
        "Gold",
        "Stone"
    );
    println!("{:-<12}-+-{:-<20}-+-{:-<10}-+-{:-<28}-+-{:-<28}-+-{:-<22}-+-{:-<5}-+-{:-<5}-+-{:-<5}-+-{:-<5}", "", "", "", "", "", "", "", "", "", "");

    for event in &events {
        let p_id = match event {
            ReplayEvent::TechResearch { player_id, .. } => *player_id,
            ReplayEvent::UnitQueued { player_id, .. } => *player_id,
            ReplayEvent::QueueCancellation { player_id, .. } => *player_id,
            ReplayEvent::BuildingConstruction { player_id, .. } => *player_id,
            ReplayEvent::Deletion { player_id, .. } => *player_id,
        };
        let p_name = player_names
            .get(&p_id)
            .map(|s| s.as_str())
            .unwrap_or("Unknown");

        match event {
            ReplayEvent::TechResearch {
                timestamp_ms,
                tech_type,
                building_id,
                building_type,
                cost,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "Research", tech_type, building_type, building_id, cost.food, cost.wood, cost.gold, cost.stone);
            }
            ReplayEvent::UnitQueued {
                timestamp_ms,
                unit_type,
                building_ids,
                building_types,
                cost,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "Training", unit_type, building_types.join(", "), building_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", "), cost.food, cost.wood, cost.gold, cost.stone);
            }
            ReplayEvent::QueueCancellation {
                timestamp_ms,
                building_ids,
                building_types,
                queue_position,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "DeQueue", format!("Pos: {}", queue_position), building_types.join(", "), building_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", "), 0, 0, 0, 0);
            }
            ReplayEvent::BuildingConstruction {
                timestamp_ms,
                building_type,
                cost,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "Build", building_type, "-", "-", cost.food, cost.wood, cost.gold, cost.stone);
            }
            ReplayEvent::Deletion {
                timestamp_ms,
                object_id,
                object_name,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "Delete", format!("{} ({})", object_name, object_id), "-", "-", 0, 0, 0, 0);
            }
        }
    }
    Ok(())
}
