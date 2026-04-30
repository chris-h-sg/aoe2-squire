use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ResourceCost {
    pub food: u32,
    pub wood: u32,
    pub gold: u32,
    pub stone: u32,
}

#[derive(Debug, Clone)]
pub enum ReplayEvent {
    UnitQueued {
        timestamp_ms: u32,
        player_id: u8,
        unit_type: String,
        cost: ResourceCost,
        building_ids: Vec<u32>,
        building_types: Vec<String>,
    },
    TechResearch {
        timestamp_ms: u32,
        player_id: u8,
        tech_type: String,
        cost: ResourceCost,
        building_id: u32,
        building_type: String,
    },
    BuildingConstruction {
        timestamp_ms: u32,
        player_id: u8,
        building_type: String,
        cost: ResourceCost,
    },
    QueueCancellation {
        timestamp_ms: u32,
        player_id: u8,
        building_ids: Vec<u32>,
        building_types: Vec<String>,
        queue_position: u32,
    },
}

fn load_tech_map() -> Result<HashMap<u32, String>, Box<dyn Error>> {
    let mut data_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    data_path.push("data");
    data_path.push("techs.csv");

    let mut rdr = csv::Reader::from_path(data_path)?;
    let mut map = HashMap::new();

    for result in rdr.records() {
        let record = result?;
        let id: u32 = record[0].parse()?;
        let name = record[1].to_string();
        map.insert(id, name);
    }

    Ok(map)
}

fn load_unit_map() -> Result<HashMap<u32, String>, Box<dyn Error>> {
    let mut data_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    data_path.push("data");
    data_path.push("units.csv");

    let mut rdr = csv::Reader::from_path(data_path)?;
    let mut map = HashMap::new();

    for result in rdr.records() {
        let record = result?;
        let id: u32 = record[0].parse()?;
        let name = record[1].to_string();
        map.insert(id, name);
    }

    Ok(map)
}

fn load_building_map() -> Result<HashMap<u32, String>, Box<dyn Error>> {
    let mut data_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    data_path.push("data");
    data_path.push("buildings.csv");

    let mut rdr = csv::Reader::from_path(data_path)?;
    let mut map = HashMap::new();

    for result in rdr.records() {
        let record = result?;
        // Ignore empty lines
        if record[0].is_empty() {
            continue;
        }
        let id: u32 = match record[0].parse() {
            Ok(v) => v,
            Err(_) => continue, // Skip header or malformed
        };
        let name = record[1].to_string();
        map.insert(id, name);
    }

    Ok(map)
}

fn format_time(ms: u32) -> String {
    let seconds = (ms / 1000) % 60;
    let minutes = (ms / 1000) / 60;
    let ms_part = ms % 1000;
    format!("{:02}:{:02}.{:03}", minutes, seconds, ms_part)
}

pub fn extract_events(path: &Path) -> Result<Vec<ReplayEvent>, Box<dyn Error>> {
    let mut events = Vec::new();

    // Load ID mappings from CSV
    let tech_map = load_tech_map()?;
    let unit_map = load_unit_map()?;
    let building_map = load_building_map()?;

    // Instance ID -> Building Name map
    let mut instance_map: HashMap<u32, String> = HashMap::new();

    let savegame = aoe2rec::Savegame::from_file(path)?;

    // Create player name map
    let mut player_names = HashMap::new();
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
        player_names.insert((i + 1) as u8, display_name);
    }

    let mut current_ms = 0;

    for op in savegame.operations {
        match op {
            aoe2rec::Operation::Sync { time_increment, .. } => {
                current_ms += time_increment;
            }
            aoe2rec::Operation::Action { action_data, .. } => match action_data {
                Some(aoe2rec::actions::ActionData::Research {
                    player_id,
                    technology_type,
                    building_id,
                    ..
                }) => {
                    let tech_name = tech_map
                        .get(&(technology_type as u32))
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Tech");

                    let b_type_name = instance_map
                        .get(&building_id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown Building".to_string());

                    events.push(ReplayEvent::TechResearch {
                        timestamp_ms: current_ms,
                        player_id,
                        tech_type: tech_name.to_string(),
                        cost: ResourceCost {
                            food: 0,
                            wood: 0,
                            gold: 0,
                            stone: 0,
                        },
                        building_id,
                        building_type: b_type_name,
                    });
                }
                Some(aoe2rec::actions::ActionData::DeQueue {
                    player_id,
                    unit_id,
                    amount,
                    building_type,
                    building_ids,
                    ..
                }) => {
                    let b_type_name = building_map
                        .get(&(building_type as u32))
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown Building")
                        .to_string();

                    for b_id in &building_ids {
                        instance_map.insert(*b_id, b_type_name.clone());
                    }

                    for _ in 0..amount {
                        let unit_name = unit_map
                            .get(&(unit_id as u32))
                            .map(|s| s.as_str())
                            .unwrap_or("Unknown Unit");

                        events.push(ReplayEvent::UnitQueued {
                            timestamp_ms: current_ms,
                            player_id,
                            unit_type: unit_name.to_string(),
                            cost: ResourceCost {
                                food: 0,
                                wood: 0,
                                gold: 0,
                                stone: 0,
                            },
                            building_ids: building_ids.clone(),
                            building_types: vec![b_type_name.clone(); building_ids.len()],
                        });
                    }
                }
                Some(aoe2rec::actions::ActionData::Order {
                    player_id,
                    order_type: aoe2rec::actions::OrderType::Unqueue,
                    unknown5,
                    object_ids,
                    ..
                }) => {
                    let building_types: Vec<String> = object_ids
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
                        building_ids: object_ids.clone(),
                        building_types,
                        queue_position: unknown5,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }

    // Display results in a table format
    println!(
        "\n{:<12} | {:<20} | {:<15} | {:<30} | {:<35} | {:<20}",
        "Time", "Player", "Event Type", "Item", "Building Type(s)", "Building ID(s)"
    );
    println!(
        "{:-<12}-+-{:-<20}-+-{:-<15}-+-{:-<30}-+-{:-<35}-+-{:-<20}",
        "", "", "", "", "", ""
    );

    for event in &events {
        match event {
            ReplayEvent::TechResearch {
                timestamp_ms,
                player_id,
                tech_type,
                building_id,
                building_type,
                ..
            } => {
                let player_name = player_names
                    .get(player_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                println!(
                    "{:<12} | {:<20} | {:<15} | {:<30} | {:<35} | {}",
                    format_time(*timestamp_ms),
                    player_name,
                    "Research",
                    tech_type,
                    building_type,
                    building_id
                );
            }
            ReplayEvent::UnitQueued {
                timestamp_ms,
                player_id,
                unit_type,
                building_ids,
                building_types,
                ..
            } => {
                let player_name = player_names
                    .get(player_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let b_types_str = building_types.join(", ");
                let b_ids_str = building_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                println!(
                    "{:<12} | {:<20} | {:<15} | {:<30} | {:<35} | {}",
                    format_time(*timestamp_ms),
                    player_name,
                    "Unit",
                    unit_type,
                    b_types_str,
                    b_ids_str
                );
            }
            ReplayEvent::QueueCancellation {
                timestamp_ms,
                player_id,
                building_ids,
                building_types,
                queue_position,
            } => {
                let player_name = player_names
                    .get(player_id)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                let b_types_str = building_types.join(", ");
                let b_ids_str = building_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                let item_desc = format!("Pos: {}", queue_position);
                println!(
                    "{:<12} | {:<20} | {:<15} | {:<30} | {:<35} | {}",
                    format_time(*timestamp_ms),
                    player_name,
                    "Unqueue",
                    item_desc,
                    b_types_str,
                    b_ids_str
                );
            }
            _ => {}
        }
    }
    println!();

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_sample_replay() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("test_bench");
        path.push("1v1-RM-sample.aoe2record");

        let savegame = aoe2rec::Savegame::from_file(&path).expect("Failed to parse savegame");
        println!(
            "Successfully parsed savegame: {} operations found",
            savegame.operations.len()
        );

        println!("\n--- Replay Metadata ---");
        println!("Build: {}", savegame.zheader.build);
        println!("Timestamp: {}", savegame.zheader.timestamp);
        println!("Num Players: {}", savegame.zheader.game_settings.n_players);

        for (i, player) in savegame.zheader.game_settings.players.iter().enumerate() {
            let name: String = player.name.clone().into();
            let ai_name: String = player.ai_name.clone().into();
            println!(
                "Slot {}: Name='{}', AI='{}', Type={}, Civ={}, Color={}",
                i + 1,
                name,
                ai_name,
                player.player_type,
                player.civ_id,
                player.color_id
            );
        }

        println!("-----------------------\n");

        extract_events(&path).expect("Failed to extract events");
    }

    #[test]
    fn test_metadata_validation() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("test_bench");
        path.push("1v1-RM-sample.aoe2record");

        let savegame = aoe2rec::Savegame::from_file(&path).expect("Failed to parse savegame");

        // Validate Player 1 (Human)
        let p1 = &savegame.zheader.game_settings.players[0];
        let p1_name: String = p1.name.clone().into();
        assert_eq!(p1_name, "Thrar");
        assert_eq!(p1.player_type, 2); // Human
        assert_eq!(p1.civ_id, 20); // Hindustanis
        assert_eq!(p1.color_id, 0); // Blue (0-indexed)

        // Validate Player 2 (AI)
        let p2 = &savegame.zheader.game_settings.players[1];
        let p2_ai_name: String = p2.ai_name.clone().into();
        assert_eq!(p2_ai_name, "Horka Bulcsú");
        assert_eq!(p2.player_type, 4); // AI
        assert_eq!(p2.civ_id, 22); // Magyars
        assert_eq!(p2.color_id, 1); // Red (0-indexed)
    }
}
