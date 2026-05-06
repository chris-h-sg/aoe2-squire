use crate::replay::ReplayEvent;
use crate::types::CsvRow;

#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub speed_factor: f64,
    pub game_start_rw_ms: f64,
    pub anchor1_ig: u64,
    pub anchor1_rw: u64,
    pub anchor2_ig: u64,
    pub anchor2_rw: u64,
}

pub fn calibrate_timing(
    csv_rows: &[CsvRow],
    events: &[ReplayEvent],
    start_ts_rw: u64,
) -> Result<Calibration, String> {
    // 1. Find starting resources from the FIRST frame (ground truth)
    let first_frame = csv_rows
        .iter()
        .find(|r| r.timestamp_ms >= start_ts_rw)
        .ok_or("No valid screen grabs found")?;
    let start_food = first_frame.food.unwrap_or(200);
    let start_wood = first_frame.wood.unwrap_or(200);

    // 2. Find first drop in CSV
    let mut anchor1_rw = 0;
    let mut anchor1_ig = 0;
    let mut first_drop_food_delta = 0;
    let mut first_drop_wood_delta = 0;

    for row in csv_rows {
        if row.timestamp_ms >= start_ts_rw
            && (row.food < Some(start_food) || row.wood < Some(start_wood))
        {
            first_drop_food_delta = start_food - row.food.unwrap_or(start_food);
            first_drop_wood_delta = start_wood - row.wood.unwrap_or(start_wood);
            anchor1_rw = row.timestamp_ms;
            break;
        }
    }

    if anchor1_rw == 0 {
        return Err("Could not find first resource drop in CSV".to_string());
    }

    // Find matching events in Replay for Anchor 1
    let mut cum_food = 0;
    let mut cum_wood = 0;
    for ev in events {
        let (ig_ms, cost) = match ev {
            ReplayEvent::TechResearch {
                timestamp_ms, cost, ..
            } => (timestamp_ms, Some(cost)),
            ReplayEvent::UnitQueued {
                timestamp_ms, cost, ..
            } => (timestamp_ms, Some(cost)),
            ReplayEvent::BuildingConstruction {
                timestamp_ms, cost, ..
            } => (timestamp_ms, Some(cost)),
            _ => (&0, None),
        };
        if let Some(c) = cost {
            cum_food += c.food;
            cum_wood += c.wood;
            if cum_food >= first_drop_food_delta && cum_wood >= first_drop_wood_delta {
                anchor1_ig = *ig_ms as u64 + 1;
                break;
            }
        }
    }

    // 3. Anchor 2: Feudal Age
    let mut anchor2_ig = 0;
    let mut anchor2_rw = 0;
    let mut feudal_ev_ig = None;
    for ev in events {
        if let ReplayEvent::TechResearch {
            timestamp_ms,
            tech_type,
            ..
        } = ev
        {
            if tech_type == "Feudal Age" {
                feudal_ev_ig = Some(*timestamp_ms);
                break;
            }
        }
    }

    if let Some(ig_ms) = feudal_ev_ig {
        // Look for 450+ food drop in CSV near expected time (using default speed 1.7 to bound search)
        let est_rw = anchor1_rw + ((ig_ms as u64 - anchor1_ig) as f64 / 1.7) as u64;
        let mut prev_food = 0;
        for row in csv_rows {
            if row.timestamp_ms > est_rw - 10000 && row.timestamp_ms < est_rw + 10000 {
                if let (Some(f), pf) = (row.food, prev_food) {
                    if pf > 450 && f < 100 && pf - f >= 450 {
                        anchor2_rw = row.timestamp_ms;
                        anchor2_ig = ig_ms as u64 + 1;
                        break;
                    }
                }
            }
            prev_food = row.food.unwrap_or(0);
        }
    }

    let mut speed_factor = 1.7;
    let game_start_rw_ms;

    if anchor2_rw > anchor1_rw {
        speed_factor = (anchor2_ig - anchor1_ig) as f64 / (anchor2_rw - anchor1_rw) as f64;
        game_start_rw_ms = anchor1_rw as f64 - (anchor1_ig as f64 / speed_factor);
    } else {
        game_start_rw_ms = anchor1_rw as f64 - (anchor1_ig as f64 / 1.7);
    }

    Ok(Calibration {
        speed_factor,
        game_start_rw_ms,
        anchor1_ig,
        anchor1_rw,
        anchor2_ig,
        anchor2_rw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::ResourceCost;

    #[test]
    fn test_basic_calibration() {
        let csv_rows = vec![
            CsvRow {
                timestamp_ms: 1000,
                food: Some(200),
                wood: Some(200),
                gold: Some(100),
                stone: Some(200),
                pop_curr: None,
                pop_max: None,
                pop_vils: None,
                idle_vils: None,
                housing: None,
            },
            CsvRow {
                timestamp_ms: 2000,
                food: Some(150),
                wood: Some(200),
                gold: Some(100),
                stone: Some(200),
                pop_curr: None,
                pop_max: None,
                pop_vils: None,
                idle_vils: None,
                housing: None,
            },
        ];
        let events = vec![ReplayEvent::UnitQueued {
            timestamp_ms: 500,
            player_id: 1,
            unit_type: "Villager".to_string(),
            cost: ResourceCost {
                food: 50,
                wood: 0,
                gold: 0,
                stone: 0,
            },
            building_ids: vec![1],
            building_types: vec!["Town Center".to_string()],
        }];

        let cal = calibrate_timing(&csv_rows, &events, 1000).unwrap();
        assert_eq!(cal.anchor1_rw, 2000);
        assert_eq!(cal.anchor1_ig, 501);
        // speed_factor defaults to 1.7 since no feudal anchor found
        assert_eq!(cal.speed_factor, 1.7);
        // game_start = 2000 - (501 / 1.7) = 2000 - 294.7 = 1705.29
        assert!((cal.game_start_rw_ms - 1705.29).abs() < 0.01);
    }
}
