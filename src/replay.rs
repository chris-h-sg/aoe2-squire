use std::path::Path;
use std::error::Error;
// use aoe2rec::Savegame; // Removed as unused

#[derive(Debug, Clone)]
pub struct ResourceCost {
    pub food: u32,
    pub wood: u32,
    pub gold: u32,
    pub stone: u32,
}

#[derive(Debug, Clone)]
pub enum ReplayEvent {
    UnitTraining {
        timestamp_ms: u32,
        unit_type: String,
        cost: ResourceCost,
    },
    TechResearch {
        timestamp_ms: u32,
        tech_type: String,
        cost: ResourceCost,
    },
    BuildingConstruction {
        timestamp_ms: u32,
        building_type: String,
        cost: ResourceCost,
    },
    QueueCancellation {
        timestamp_ms: u32,
        refund: ResourceCost,
        cancelled_type: String,
    }
}

pub fn extract_events(path: &Path) -> Result<Vec<ReplayEvent>, Box<dyn Error>> {
    let mut events = Vec::new();
    
    // Load ID mappings
    let mut data_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    data_path.push("research");
    data_path.push("aoe2_data.json");
    let data_str = std::fs::read_to_string(data_path)?;
    let id_map: serde_json::Value = serde_json::from_str(&data_str)?;
    
    let savegame = aoe2rec::Savegame::from_file(path)?;
    
    let mut current_ms = 0;
    
    for op in savegame.operations {
        match op {
            aoe2rec::Operation::Sync { time_increment, .. } => {
                current_ms += time_increment;
            }
            aoe2rec::Operation::Action { action_data, .. } => {
                if let Some(aoe2rec::actions::ActionData::Research { player_id, technology_type, .. }) = action_data {
                    let tech_name = id_map["technologies"][technology_type.to_string()]
                        .as_str()
                        .unwrap_or("Unknown Tech");
                    
                    let seconds = (current_ms / 1000) % 60;
                    let minutes = (current_ms / 1000) / 60;
                    let ms = current_ms % 1000;
                    
                    println!("[{:02}:{:02}.{:03}] Player {}: Researching {}", minutes, seconds, ms, player_id, tech_name);
                    
                    events.push(ReplayEvent::TechResearch {
                        timestamp_ms: current_ms,
                        tech_type: tech_name.to_string(),
                        cost: ResourceCost { food: 0, wood: 0, gold: 0, stone: 0 },
                    });
                }
            }
            _ => {}
        }
    }



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
        println!("Successfully parsed savegame: {} operations found", savegame.operations.len());
        
        println!("\n--- Replay Metadata ---");
        println!("Build: {}", savegame.zheader.build);
        println!("Timestamp: {}", savegame.zheader.timestamp);
        println!("Num Players: {}", savegame.zheader.game_settings.n_players);
        
        for (i, player) in savegame.zheader.game_settings.players.iter().enumerate() {
            let name: String = player.name.clone().into();
            let ai_name: String = player.ai_name.clone().into();
            println!("Slot {}: Name='{}', AI='{}', Type={}, Civ={}, Color={}", 
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
        assert_eq!(p1.player_type, 2);    // Human
        assert_eq!(p1.civ_id, 20);       // Hindustanis
        assert_eq!(p1.color_id, 0);      // Blue (0-indexed)
        
        // Validate Player 2 (AI)
        let p2 = &savegame.zheader.game_settings.players[1];
        let p2_ai_name: String = p2.ai_name.clone().into();
        assert_eq!(p2_ai_name, "Horka Bulcsú");
        assert_eq!(p2.player_type, 4);    // AI
        assert_eq!(p2.civ_id, 22);       // Magyars
        assert_eq!(p2.color_id, 1);      // Red (0-indexed)
    }
}
