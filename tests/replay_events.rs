//! Replay event extraction on the sample game and other test replays.
mod common;

use aoe2_squire::replay::{extract_events, ReplayEvent};
use std::collections::HashMap;

#[test]
fn queued_units_cost_one_unit_each_regardless_of_selected_buildings() {
    let data = extract_events(&common::test_bench("1v1-RM-sample.aoe2record")).unwrap();

    let mut first_cost: HashMap<&str, (u32, u32, u32, u32)> = HashMap::new();
    let mut multi_building = 0;
    for ev in &data.events {
        if let ReplayEvent::UnitQueued {
            unit_type,
            cost,
            building_ids,
            ..
        } = ev
        {
            multi_building += (building_ids.len() > 1) as usize;
            let c = (cost.food, cost.wood, cost.gold, cost.stone);
            let first = *first_cost.entry(unit_type).or_insert(c);
            assert_eq!(
                c,
                first,
                "{} queued with {} buildings selected costs {:?}, another queue of it {:?}",
                unit_type,
                building_ids.len(),
                c,
                first
            );
        }
    }
    assert!(multi_building > 0, "the sample has multi-building queues");

    // At 24:16.178 two archery ranges are selected for one Cavalry Archer; on screen gold
    // drops by 60 per click and the production overview shows one unit per click.
    let cav_archer: Vec<(u32, u32, usize)> = data
        .events
        .iter()
        .filter_map(|e| match e {
            ReplayEvent::UnitQueued {
                timestamp_ms: 1_456_178,
                unit_type,
                cost,
                building_ids,
                ..
            } if unit_type == "Cavalry Archer" => Some((cost.wood, cost.gold, building_ids.len())),
            _ => None,
        })
        .collect();
    assert_eq!(cav_archer, vec![(40, 60, 2)]);
}

/// A short game against the AI at Fast speed with deleted buildings. Games against the AI at
/// Fast speed failed to parse until the aoe2rec fix of the AI header skip.
#[test]
fn deletions_sample_parses_with_its_deletions() {
    let data = extract_events(&common::test_bench("deletions-sample.aoe2record")).unwrap();
    assert!(data.unknown_ids.is_empty(), "{:?}", data.unknown_ids);
    let deletions: Vec<&str> = data
        .events
        .iter()
        .filter_map(|e| match e {
            ReplayEvent::Deletion { object_name, .. } => Some(object_name.as_str()),
            _ => None,
        })
        .collect();
    assert!(!deletions.is_empty(), "no deletions extracted");
}
