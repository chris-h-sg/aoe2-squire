use crate::game_data::{GameData, Kind, FARM_BUILDING};
use crate::types::Age;
use aoe2rec::Savegame;
use binrw::BinReaderExt;
use chrono::{Local, TimeZone};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
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

#[derive(Debug, Serialize, Clone)]
pub enum ReplayEvent {
    TechResearch {
        timestamp_ms: u32,
        player_id: u8,
        tech_id: u32,
        tech_type: String,
        cost: ResourceCost,
        building_id: u32,
        building_type: String,
    },
    UnitQueued {
        timestamp_ms: u32,
        player_id: u8,
        unit_id: u32,
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
    /// A spend without a unit, building or technology: a farm queued at a mill
    /// (`FARM_QUEUE_DESC`) or a market sale (`MARKET_SELL_PREFIX` + resource).
    OtherSpend {
        timestamp_ms: u32,
        player_id: u8,
        description: String,
        cost: ResourceCost,
        /// Whether the player also receives resources (the gold of a market sale).
        gives_resources: bool,
    },
}

/// Description of a farm queued at a mill (paid when queued, built later automatically).
pub const FARM_QUEUE_DESC: &str = "Mill Queue Farm";
/// Description prefix of a market sale, followed by the resource sold.
pub const MARKET_SELL_PREFIX: &str = "Market Sell ";

impl ReplayEvent {
    pub fn timestamp_ms(&self) -> u32 {
        match self {
            ReplayEvent::TechResearch { timestamp_ms, .. }
            | ReplayEvent::UnitQueued { timestamp_ms, .. }
            | ReplayEvent::QueueCancellation { timestamp_ms, .. }
            | ReplayEvent::BuildingConstruction { timestamp_ms, .. }
            | ReplayEvent::Deletion { timestamp_ms, .. }
            | ReplayEvent::OtherSpend { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    pub fn player_id(&self) -> u8 {
        match self {
            ReplayEvent::TechResearch { player_id, .. }
            | ReplayEvent::UnitQueued { player_id, .. }
            | ReplayEvent::QueueCancellation { player_id, .. }
            | ReplayEvent::BuildingConstruction { player_id, .. }
            | ReplayEvent::Deletion { player_id, .. }
            | ReplayEvent::OtherSpend { player_id, .. } => *player_id,
        }
    }

    /// What the command costs; None for commands that spend nothing.
    pub fn cost(&self) -> Option<&ResourceCost> {
        match self {
            ReplayEvent::TechResearch { cost, .. }
            | ReplayEvent::UnitQueued { cost, .. }
            | ReplayEvent::BuildingConstruction { cost, .. }
            | ReplayEvent::OtherSpend { cost, .. } => Some(cost),
            ReplayEvent::QueueCancellation { .. } | ReplayEvent::Deletion { .. } => None,
        }
    }

    /// Short description, e.g. "Research Feudal Age", "Queue Villager", "Build House".
    pub fn description(&self) -> String {
        match self {
            ReplayEvent::TechResearch { tech_type, .. } => format!("Research {}", tech_type),
            ReplayEvent::UnitQueued { unit_type, .. } => format!("Queue {}", unit_type),
            ReplayEvent::QueueCancellation { .. } => "Unqueue".to_string(),
            ReplayEvent::BuildingConstruction { building_type, .. } => {
                format!("Build {}", building_type)
            }
            ReplayEvent::Deletion { object_name, .. } => format!("Delete {}", object_name),
            ReplayEvent::OtherSpend { description, .. } => description.clone(),
        }
    }

    /// The building(s) involved, comma-separated for queues across several buildings.
    pub fn building_type(&self) -> String {
        match self {
            ReplayEvent::TechResearch { building_type, .. }
            | ReplayEvent::BuildingConstruction { building_type, .. } => building_type.clone(),
            ReplayEvent::UnitQueued { building_types, .. }
            | ReplayEvent::QueueCancellation { building_types, .. } => building_types.join(","),
            ReplayEvent::Deletion { .. } | ReplayEvent::OtherSpend { .. } => String::new(),
        }
    }

    /// Ids of the buildings the command was given to (none for construction and deletion).
    pub fn building_ids(&self) -> Vec<u32> {
        match self {
            ReplayEvent::TechResearch { building_id, .. } => vec![*building_id],
            ReplayEvent::UnitQueued { building_ids, .. }
            | ReplayEvent::QueueCancellation { building_ids, .. } => building_ids.clone(),
            ReplayEvent::BuildingConstruction { .. }
            | ReplayEvent::Deletion { .. }
            | ReplayEvent::OtherSpend { .. } => vec![],
        }
    }

    /// The technology, unit or building ID the command refers to (see `game_data`).
    pub fn type_id(&self) -> Option<u32> {
        match self {
            ReplayEvent::TechResearch { tech_id, .. } => Some(*tech_id),
            ReplayEvent::UnitQueued { unit_id, .. } => Some(*unit_id),
            ReplayEvent::BuildingConstruction {
                building_type_id, ..
            } => Some(*building_type_id),
            ReplayEvent::QueueCancellation { .. }
            | ReplayEvent::Deletion { .. }
            | ReplayEvent::OtherSpend { .. } => None,
        }
    }

    /// The age this command starts researching, if it is an age-up click.
    pub fn age_researched(&self) -> Option<Age> {
        match self {
            ReplayEvent::TechResearch { tech_id, .. } => Age::from_tech_id(*tech_id),
            _ => None,
        }
    }

    /// Commands that give resources back: queue cancellations, deletions and market sales.
    pub fn is_refund(&self) -> bool {
        matches!(
            self,
            ReplayEvent::QueueCancellation { .. }
                | ReplayEvent::Deletion { .. }
                | ReplayEvent::OtherSpend {
                    gives_resources: true,
                    ..
                }
        )
    }
}

pub fn format_time(ms: u32) -> String {
    let seconds = (ms as f32) / 1000.0;
    let minutes = (seconds / 60.0).floor() as u32;
    let remaining_seconds = seconds % 60.0;
    format!("{:02}:{:06.3}", minutes, remaining_seconds)
}

/// `id,name,...` rows of an embedded CSV (civilizations, maps) as id -> name.
fn parse_id_names(content: &str) -> Result<HashMap<u32, String>, Box<dyn std::error::Error>> {
    let mut names = HashMap::new();
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());
    for record in rdr.records() {
        let record = record?;
        if let (Some(Ok(id)), Some(name)) = (record.get(0).map(|v| v.trim().parse()), record.get(1))
        {
            names.insert(id, name.to_string());
        }
    }
    Ok(names)
}

/// Looks up command IDs in the game data and counts the ones it does not know.
struct Lookup {
    data: &'static GameData,
    unknown: BTreeMap<(&'static str, u32), usize>,
}

impl Lookup {
    fn name_and_cost(&mut self, kind: Kind, id: u32) -> (String, ResourceCost) {
        match self.data.lookup(kind, id) {
            Some(entry) => (entry.name.clone(), entry.cost.clone()),
            None => {
                *self.unknown.entry((kind.as_str(), id)).or_default() += 1;
                (
                    format!("Unknown {} {}", kind.as_str(), id),
                    ResourceCost {
                        food: 0,
                        wood: 0,
                        gold: 0,
                        stone: 0,
                    },
                )
            }
        }
    }

    fn name(&mut self, kind: Kind, id: u32) -> String {
        self.name_and_cost(kind, id).0
    }
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
    pub map_name: String,
}

pub struct ReplayData {
    pub rec_player: u32,
    pub player_names: HashMap<u8, String>,
    pub events: Vec<ReplayEvent>,
    pub metadata: MatchMetadata,
    /// Unit, building and technology IDs in commands that `data/game_data.csv` does not
    /// know, as ("unit" | "building" | "tech", id) -> number of commands.
    pub unknown_ids: BTreeMap<(&'static str, u32), usize>,
}

pub fn extract_events(replay_path: &Path) -> Result<ReplayData, Box<dyn std::error::Error>> {
    let file = File::open(replay_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    let mut cursor = std::io::Cursor::new(&buffer);
    // Fully qualified: std may add a `read_le` method that would shadow binrw's.
    let mut savegame: Savegame = BinReaderExt::read_le(&mut cursor)?;
    // All chapters' operations in order, owned (what `Savegame::operations()` iterates by reference).
    let operations: Vec<aoe2rec::Operation> = savegame
        .chapters
        .iter_mut()
        .flat_map(|chapter| std::mem::take(&mut chapter.operations))
        .collect();
    let header = savegame
        .header()
        .ok_or("replay contains no recorded chapters")?;

    println!("Replay parsed successfully!");
    println!("Build: {:?}", header.build);

    // Load reference data
    let mut lookup = Lookup {
        data: GameData::get(),
        unknown: BTreeMap::new(),
    };
    let civ_map = parse_id_names(include_str!("../data/civilizations.csv"))?;
    let map_map = parse_id_names(include_str!("../data/maps.csv"))?;

    let mut events = Vec::new();
    let mut instance_map: HashMap<u32, String> = HashMap::new();
    let mut pending_builds: HashMap<(i32, i32), String> = HashMap::new();

    for obj in &header.initial.initial_tail.initial_object_instances {
        // Try to identify the object name from buildings or units. Starting objects include
        // map objects (trees, mines, animals), so misses are not counted as unknown IDs.
        let type_id = obj.object_type_id as u32;
        let name = lookup
            .data
            .lookup(Kind::Building, type_id)
            .or_else(|| lookup.data.lookup(Kind::Unit, type_id))
            .map(|e| e.name.clone())
            .unwrap_or_else(|| format!("Unknown ({})", obj.object_type_id));

        instance_map.insert(obj.object_id, name.clone());
    }

    let mut player_names = HashMap::new();
    let mut player_infos = Vec::new();

    for (i, player) in header.game_settings.players.iter().enumerate() {
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
    let pov_id = header.replay.rec_player as u8;
    if let Some(pov_name) = player_names.get(&pov_id) {
        if let Some(pos) = player_infos.iter().position(|p| &p.name == pov_name) {
            let pov_info = player_infos.remove(pos);
            player_infos.insert(0, pov_info);
        }
    }

    let mut current_ms = 0;
    let mut loser_id: Option<u8> = None;

    for op in operations {
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
                    let tech_id = technology_type as u32;
                    let (tech_name, cost) = lookup.name_and_cost(Kind::Tech, tech_id);

                    let b_type_name = instance_map
                        .get(&building_id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown Building".to_string());

                    events.push(ReplayEvent::TechResearch {
                        timestamp_ms: current_ms,
                        player_id,
                        tech_id,
                        tech_type: tech_name,
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
                    let b_type_name = lookup.name(Kind::Building, building_type as u32);

                    for b_id in &building_ids {
                        instance_map.insert(*b_id, b_type_name.clone());
                    }

                    let (unit_name, base_cost) = lookup.name_and_cost(Kind::Unit, unit_id);

                    // `amount` is the total number of units, spread across the selected
                    // buildings, so each queued unit costs the base cost once.
                    for _ in 0..amount {
                        events.push(ReplayEvent::UnitQueued {
                            timestamp_ms: current_ms,
                            player_id,
                            unit_id,
                            unit_type: unit_name.clone(),
                            cost: base_cost.clone(),
                            building_ids: building_ids.clone(),
                            building_types: vec![b_type_name.clone(); building_ids.len()],
                        });
                    }
                }
                aoe2rec::actions::ActionData::Order {
                    player_id,
                    order_type,
                    object_ids,
                    unknown5,
                    ..
                } => {
                    let building_ids = object_ids;

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

                    let b_type_name = if !building_ids.is_empty() {
                        instance_map
                            .get(&building_ids[0])
                            .cloned()
                            .unwrap_or_else(|| "Unknown Building".to_string())
                    } else {
                        "Unknown Building".to_string()
                    };

                    let (unit_name, base_cost) = lookup.name_and_cost(Kind::Unit, unit_id);

                    // `count` is the total number of units, spread across the selected
                    // buildings, so each queued unit costs the base cost once.
                    for _ in 0..count {
                        events.push(ReplayEvent::UnitQueued {
                            timestamp_ms: current_ms,
                            player_id,
                            unit_id,
                            unit_type: unit_name.clone(),
                            cost: base_cost.clone(),
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
                    let (b_type_name, cost) =
                        lookup.name_and_cost(Kind::Building, building_type_id);

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
                aoe2rec::actions::ActionData::Game {
                    player_id,
                    game_command: aoe2rec::actions::Game::FarmQueue {},
                    ..
                } => {
                    let (_, cost) = lookup.name_and_cost(Kind::Building, FARM_BUILDING);
                    events.push(ReplayEvent::OtherSpend {
                        timestamp_ms: current_ms,
                        player_id,
                        description: FARM_QUEUE_DESC.to_string(),
                        cost,
                        gives_resources: false,
                    });
                }
                aoe2rec::actions::ActionData::Sell {
                    player_id, data, ..
                } => {
                    // data: resource (0 food, 1 wood, 2 stone), unknown, amount in hundreds,
                    // unknown, market object id (u32).
                    if let (Some(&resource), Some(&hundreds)) = (data.first(), data.get(2)) {
                        let amount = 100 * hundreds as u32;
                        let mut cost = ResourceCost {
                            food: 0,
                            wood: 0,
                            gold: 0,
                            stone: 0,
                        };
                        let name = match resource {
                            0 => {
                                cost.food = amount;
                                "Food"
                            }
                            1 => {
                                cost.wood = amount;
                                "Wood"
                            }
                            2 => {
                                cost.stone = amount;
                                "Stone"
                            }
                            _ => "Unknown",
                        };
                        events.push(ReplayEvent::OtherSpend {
                            timestamp_ms: current_ms,
                            player_id,
                            description: format!("{}{}", MARKET_SELL_PREFIX, name),
                            cost,
                            gives_resources: true,
                        });
                    }
                }
                aoe2rec::actions::ActionData::Wall {
                    player_id, data, ..
                } => {
                    // data: selected count (u32), start x, y and end x, y (u16 each), wall
                    // building id (u32), ...
                    if data.len() >= 16 {
                        let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]) as i32;
                        let (x1, y1, x2, y2) = (u16_at(4), u16_at(6), u16_at(8), u16_at(10));
                        let wall_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                        // Walls follow the line tile by tile, diagonally where needed. Blocked
                        // tiles are skipped and not paid for, so this is an upper bound.
                        let tiles = (x2 - x1).abs().max((y2 - y1).abs()) as u32 + 1;
                        let (name, per_tile) = lookup.name_and_cost(Kind::Building, wall_id);
                        events.push(ReplayEvent::BuildingConstruction {
                            timestamp_ms: current_ms,
                            player_id,
                            building_type: name,
                            building_type_id: wall_id,
                            x: x1 as f32,
                            y: y1 as f32,
                            cost: ResourceCost {
                                food: per_tile.food * tiles,
                                wood: per_tile.wood * tiles,
                                gold: per_tile.gold * tiles,
                                stone: per_tile.stone * tiles,
                            },
                        });
                    }
                }
                aoe2rec::actions::ActionData::Resign { player_id, .. } => {
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

    let datetime = Local.timestamp_opt(header.timestamp as i64, 0).unwrap();
    let start_time = datetime.format("%Y-%m-%d %H:%M:%S").to_string();

    let (rec_player_name, rec_player_civ) = player_names
        .get(&(header.replay.rec_player as u8))
        .map(|name| {
            let civ = player_infos
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.civ.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            (name.clone(), civ)
        })
        .unwrap_or_else(|| ("Unknown".to_string(), "Unknown".to_string()));

    let resolved_map_id = header.game_settings.resolved_map_id;
    let map_name = map_map
        .get(&resolved_map_id)
        .cloned()
        .unwrap_or_else(|| format!("Unknown Map ({})", resolved_map_id));

    let metadata = MatchMetadata {
        start_time,
        duration_formatted: format_time(current_ms),
        duration_sec: current_ms as f64 / 1000.0,
        players: player_infos,
        rec_player_name,
        rec_player_civ,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        has_replay: true,
        map_name,
    };

    if !lookup.unknown.is_empty() {
        let list: Vec<String> = lookup
            .unknown
            .iter()
            .map(|((kind, id), n)| format!("{} {} ({}x)", kind, id, n))
            .collect();
        println!(
            "Unknown IDs (not in data/game_data.csv, no name or cost): {}",
            list.join(", ")
        );
    }

    Ok(ReplayData {
        rec_player: header.replay.rec_player as u32,
        player_names,
        events,
        metadata,
        unknown_ids: lookup.unknown,
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
        let p_id = event.player_id();
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
            ReplayEvent::OtherSpend {
                timestamp_ms,
                description,
                cost,
                ..
            } => {
                println!("{:<12} | {:<20} | {:<10} | {:<28} | {:<28} | {:<22} | {:>5} | {:>5} | {:>5} | {:>5}", format_time(*timestamp_ms), p_name, "Spend", description, "-", "-", cost.food, cost.wood, cost.gold, cost.stone);
            }
        }
    }
    Ok(())
}
