use crate::constants::DEFAULT_MERGED_CSV;
use crate::types::{Age, CsvRow, HousingStatus, Observation};
use csv::Reader;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEndReason {
    ValueChanged,
    AgeResearch(Age),
    EndOfData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameSegment<T> {
    pub start_ms: u64,
    pub end_ms: u64,
    pub value: T,
    pub age: Age,
    pub end_reason: SegmentEndReason,
}

pub type VsSegment = GameSegment<u32>;
pub type HousingSegment = GameSegment<HousingStatus>;

pub fn load_merged_observations() -> Result<Reader<File>, Box<dyn Error>> {
    let path = DEFAULT_MERGED_CSV;
    if !Path::new(path).exists() {
        return Err(format!("{} not found. Run mesher first.", path).into());
    }
    Ok(csv::ReaderBuilder::new().from_path(path)?)
}

/// In-game time of the last age-up click per age. Only the last click can be the one that
/// went through: clicking the same age again means the earlier research was cancelled.
pub fn last_age_up_clicks(rows: &[Observation]) -> HashMap<Age, u64> {
    let mut clicks = HashMap::new();
    for event in rows.iter().filter_map(Observation::event) {
        if let Some(age) = event.age_researched() {
            clicks.insert(age, event.timestamp_ms() as u64);
        }
    }
    clicks
}

/// The age an observation starts researching, if it is the last click for that age.
pub fn effective_age_up(obs: &Observation, last_clicks: &HashMap<Age, u64>) -> Option<Age> {
    obs.event()?
        .age_researched()
        .filter(|age| last_clicks.get(age) == Some(&obs.in_game_ms()))
}

/// Splits the timeline into runs of the same extracted value, also cutting at each age-up
/// (the last click per age, see `last_age_up_clicks`). Readings for which `extractor`
/// returns None are skipped.
pub fn segment_game<T, F>(rows: &[Observation], extractor: F) -> Vec<GameSegment<T>>
where
    T: PartialEq + Clone,
    F: Fn(&CsvRow) -> Option<T>,
{
    let last_clicks = last_age_up_clicks(rows);
    let mut segments = Vec::new();
    let mut current_value: Option<T> = None;
    let mut segment_start_ms: u64 = 0;
    let mut last_ig_ms: u64 = 0;
    let mut current_age = Age::Dark;

    for row in rows {
        last_ig_ms = row.in_game_ms();
        let ig_ms = row.in_game_ms();

        match row {
            Observation::Screen { reading, .. } => {
                let new_value = extractor(reading);
                if new_value.is_none() {
                    continue;
                }

                match current_value {
                    Some(ref old_val) if new_value.as_ref() != Some(old_val) => {
                        segments.push(GameSegment {
                            start_ms: segment_start_ms,
                            end_ms: ig_ms,
                            value: old_val.clone(),
                            age: current_age,
                            end_reason: SegmentEndReason::ValueChanged,
                        });
                        segment_start_ms = ig_ms;
                        current_value = new_value;
                    }
                    None => {
                        segment_start_ms = ig_ms;
                        current_value = new_value;
                    }
                    _ => {}
                }
            }
            Observation::Event(_) => {
                if let Some(age) = effective_age_up(row, &last_clicks) {
                    if let Some(ref val) = current_value {
                        segments.push(GameSegment {
                            start_ms: segment_start_ms,
                            end_ms: ig_ms,
                            value: val.clone(),
                            age: current_age,
                            end_reason: SegmentEndReason::AgeResearch(age),
                        });
                    }
                    current_age = age;
                    segment_start_ms = ig_ms;
                }
            }
        }
    }

    // Push the final segment up to the last known timestamp
    if let Some(val) = current_value {
        segments.push(GameSegment {
            start_ms: segment_start_ms,
            end_ms: last_ig_ms,
            value: val,
            age: current_age,
            end_reason: SegmentEndReason::EndOfData,
        });
    }
    segments
}

pub fn calculate_vs_lost(segments: &[VsSegment]) -> HashMap<String, f64> {
    let mut results = HashMap::new();
    for seg in segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let vs_lost = duration_sec * seg.value as f64;
        *results.entry(seg.age.name().to_string()).or_insert(0.0) += vs_lost;
    }
    results
}

pub fn calculate_housing_metrics(
    segments: &[HousingSegment],
) -> HashMap<String, HashMap<String, f64>> {
    let mut results = HashMap::new();
    for seg in segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let age_map = results
            .entry(seg.age.name().to_string())
            .or_insert_with(HashMap::new);
        *age_map.entry(seg.value.as_str().to_string()).or_insert(0.0) += duration_sec;
    }
    results
}

pub fn format_time(ms: u64) -> String {
    let minutes = ms / 60000;
    let seconds = (ms % 60000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}.{:03}", minutes, seconds, millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MergedRow;

    fn mock_row(ms: u64, obs: &str, val: &str, age_desc: &str) -> Observation {
        Observation::from_merged_row(&MergedRow {
            in_game_ms: ms,
            observation_type: obs.to_string(),
            event_desc: age_desc.to_string(),
            idle_vils: val.to_string(),
            housing: val.to_string(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn test_generic_segmentation() {
        let rows = vec![
            mock_row(1000, "ScreenGrab", "3", ""),
            mock_row(2000, "ScreenGrab", "3", ""),
            mock_row(3000, "RecEvent", "", "Research Feudal Age"),
            mock_row(4000, "ScreenGrab", "1", ""),
        ];

        let segments = segment_game(&rows, |r| r.idle_vils);
        assert_eq!(segments.len(), 3);

        assert_eq!(segments[0].value, 3);
        assert_eq!(segments[0].age, Age::Dark);
        assert_eq!(segments[0].end_ms, 3000);
        assert_eq!(
            segments[0].end_reason,
            SegmentEndReason::AgeResearch(Age::Feudal)
        );

        assert_eq!(segments[1].value, 3);
        assert_eq!(segments[1].age, Age::Feudal);
        assert_eq!(segments[1].start_ms, 3000);
        assert_eq!(segments[1].end_ms, 4000);
        assert_eq!(segments[1].end_reason, SegmentEndReason::ValueChanged);

        assert_eq!(segments[2].value, 1);
        assert_eq!(segments[2].age, Age::Feudal);
        assert_eq!(segments[2].start_ms, 4000);
        assert_eq!(segments[2].end_ms, 4000);
        assert_eq!(segments[2].end_reason, SegmentEndReason::EndOfData);
    }

    #[test]
    fn test_housing_segmentation() {
        let rows = vec![
            mock_row(1000, "ScreenGrab", "normal", ""),
            mock_row(2000, "ScreenGrab", "housed", ""),
            mock_row(3000, "ScreenGrab", "housed", ""),
            mock_row(4000, "ScreenGrab", "queued", ""),
        ];

        let segments = segment_game(&rows, |r| r.housing);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].value, HousingStatus::Normal);
        assert_eq!(segments[1].value, HousingStatus::Housed);
        assert_eq!(segments[2].value, HousingStatus::Queued);
    }

    #[test]
    fn test_vs_calculation() {
        let segments = vec![
            VsSegment {
                start_ms: 0,
                end_ms: 10000,
                value: 3,
                age: Age::Dark,
                end_reason: SegmentEndReason::ValueChanged,
            },
            VsSegment {
                start_ms: 10000,
                end_ms: 20000,
                value: 1,
                age: Age::Dark,
                end_reason: SegmentEndReason::EndOfData,
            },
        ];
        let results = calculate_vs_lost(&segments);
        assert_eq!(*results.get("Dark Age").unwrap(), 40.0);
    }

    #[test]
    fn test_only_the_last_age_up_click_counts() {
        // Feudal clicked three times; the first two were cancelled.
        let rows = vec![
            mock_row(1000, "ScreenGrab", "2", ""),
            mock_row(2000, "RecEvent", "", "Research Feudal Age"),
            mock_row(3000, "RecEvent", "", "Research Feudal Age"),
            mock_row(4000, "ScreenGrab", "2", ""),
            mock_row(5000, "RecEvent", "", "Research Feudal Age"),
            mock_row(6000, "ScreenGrab", "2", ""),
        ];

        assert_eq!(last_age_up_clicks(&rows).get(&Age::Feudal), Some(&5000));
        let segments = segment_game(&rows, |r| r.idle_vils);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].age, Age::Dark);
        assert_eq!((segments[0].start_ms, segments[0].end_ms), (1000, 5000));
        assert_eq!(
            segments[0].end_reason,
            SegmentEndReason::AgeResearch(Age::Feudal)
        );
        assert_eq!(segments[1].age, Age::Feudal);
        assert_eq!((segments[1].start_ms, segments[1].end_ms), (5000, 6000));
    }

    #[test]
    fn test_age_transition_boundary() {
        // Case: Age transition happens at the exact same MS as a value change
        let rows = vec![
            mock_row(1000, "ScreenGrab", "3", ""),
            mock_row(2000, "RecEvent", "", "Research Feudal Age"),
            mock_row(2000, "ScreenGrab", "1", ""),
            mock_row(3000, "ScreenGrab", "1", ""),
        ];

        let segments = segment_game(&rows, |r| r.idle_vils);

        // Expected segments:
        // 1. [1000, 2000] value 3, Dark Age, AgeResearch
        // 2. [2000, 2000] value 3, Feudal Age, ValueChanged
        // 3. [2000, 3000] value 1, Feudal Age, EndOfData

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].age, Age::Dark);
        assert_eq!(segments[0].value, 3);
        assert_eq!(
            segments[0].end_reason,
            SegmentEndReason::AgeResearch(Age::Feudal)
        );

        assert_eq!(segments[1].age, Age::Feudal);
        assert_eq!(segments[1].value, 3);
        assert_eq!(segments[1].start_ms, 2000);
        assert_eq!(segments[1].end_ms, 2000);
        assert_eq!(segments[1].end_reason, SegmentEndReason::ValueChanged);

        assert_eq!(segments[2].age, Age::Feudal);
        assert_eq!(segments[2].value, 1);
        assert_eq!(segments[2].start_ms, 2000);
        assert_eq!(segments[2].end_ms, 3000);
        assert_eq!(segments[2].end_reason, SegmentEndReason::EndOfData);
    }
}

pub mod floating;
pub mod housing;
pub mod idle;
pub mod mesher;
pub mod run;
pub mod smoothing;
