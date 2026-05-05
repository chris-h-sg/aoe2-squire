use crate::constants::DEFAULT_MERGED_CSV;
use crate::types::MergedRow;
use csv::Reader;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEndReason {
    ValueChanged,
    AgeResearch(String),
    EndOfData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameSegment<T> {
    pub start_ms: u64,
    pub end_ms: u64,
    pub value: T,
    pub age: String,
    pub end_reason: SegmentEndReason,
}

pub type VsSegment = GameSegment<u32>;
pub type HousingSegment = GameSegment<String>;

pub fn load_merged_observations() -> Result<Reader<File>, Box<dyn Error>> {
    let path = DEFAULT_MERGED_CSV;
    if !Path::new(path).exists() {
        return Err(format!("{} not found. Run mesher first.", path).into());
    }
    Ok(csv::ReaderBuilder::new().from_path(path)?)
}

pub fn segment_game<T, F>(
    rows: impl Iterator<Item = MergedRow>,
    extractor: F,
) -> Vec<GameSegment<T>>
where
    T: PartialEq + Clone,
    F: Fn(&MergedRow) -> Option<T>,
{
    let mut segments = Vec::new();
    let mut current_value: Option<T> = None;
    let mut segment_start_ms: u64 = 0;
    let mut last_ig_ms: u64 = 0;
    let mut current_age = "Dark Age".to_string();

    for row in rows {
        last_ig_ms = row.in_game_ms;
        let ig_ms = row.in_game_ms;

        if row.observation_type == "ScreenGrab" {
            let new_value = extractor(&row);
            if new_value.is_none() {
                continue;
            }

            match current_value {
                Some(ref old_val) if new_value.as_ref() != Some(old_val) => {
                    segments.push(GameSegment {
                        start_ms: segment_start_ms,
                        end_ms: ig_ms,
                        value: old_val.clone(),
                        age: current_age.clone(),
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
        } else if row.observation_type == "RecEvent" {
            let desc = &row.event_desc;
            let new_age_name = if desc == "Research Feudal Age" {
                Some("Feudal Age")
            } else if desc == "Research Castle Age" {
                Some("Castle Age")
            } else if desc == "Research Imperial Age" {
                Some("Imperial Age")
            } else {
                None
            };

            if let Some(age_name) = new_age_name {
                if let Some(ref val) = current_value {
                    segments.push(GameSegment {
                        start_ms: segment_start_ms,
                        end_ms: ig_ms,
                        value: val.clone(),
                        age: current_age.clone(),
                        end_reason: SegmentEndReason::AgeResearch(age_name.to_string()),
                    });
                }
                current_age = age_name.to_string();
                segment_start_ms = ig_ms;
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
        *results.entry(seg.age.clone()).or_insert(0.0) += vs_lost;
    }
    results
}

pub fn calculate_housing_metrics(
    segments: &[HousingSegment],
) -> HashMap<String, HashMap<String, f64>> {
    let mut results = HashMap::new();
    for seg in segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let age_map = results.entry(seg.age.clone()).or_insert_with(HashMap::new);
        *age_map.entry(seg.value.clone()).or_insert(0.0) += duration_sec;
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

    fn mock_row(ms: u64, obs: &str, val: &str, age_desc: &str) -> MergedRow {
        MergedRow {
            in_game_ms: ms,
            observation_type: obs.to_string(),
            event_desc: age_desc.to_string(),
            idle_vils: val.to_string(),
            housing: val.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_generic_segmentation() {
        let rows = vec![
            mock_row(1000, "ScreenGrab", "3", ""),
            mock_row(2000, "ScreenGrab", "3", ""),
            mock_row(3000, "RecEvent", "", "Research Feudal Age"),
            mock_row(4000, "ScreenGrab", "1", ""),
        ];

        let segments = segment_game(rows.into_iter(), |r| r.idle_vils.parse::<u32>().ok());
        assert_eq!(segments.len(), 3);

        assert_eq!(segments[0].value, 3);
        assert_eq!(segments[0].age, "Dark Age");
        assert_eq!(segments[0].end_ms, 3000);
        assert_eq!(
            segments[0].end_reason,
            SegmentEndReason::AgeResearch("Feudal Age".to_string())
        );

        assert_eq!(segments[1].value, 3);
        assert_eq!(segments[1].age, "Feudal Age");
        assert_eq!(segments[1].start_ms, 3000);
        assert_eq!(segments[1].end_ms, 4000);
        assert_eq!(segments[1].end_reason, SegmentEndReason::ValueChanged);

        assert_eq!(segments[2].value, 1);
        assert_eq!(segments[2].age, "Feudal Age");
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

        let segments = segment_game(rows.into_iter(), |r| Some(r.housing.clone()));
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].value, "normal");
        assert_eq!(segments[1].value, "housed");
        assert_eq!(segments[2].value, "queued");
    }

    #[test]
    fn test_vs_calculation() {
        let segments = vec![
            VsSegment {
                start_ms: 0,
                end_ms: 10000,
                value: 3,
                age: "Dark Age".to_string(),
                end_reason: SegmentEndReason::ValueChanged,
            },
            VsSegment {
                start_ms: 10000,
                end_ms: 20000,
                value: 1,
                age: "Dark Age".to_string(),
                end_reason: SegmentEndReason::EndOfData,
            },
        ];
        let results = calculate_vs_lost(&segments);
        assert_eq!(*results.get("Dark Age").unwrap(), 40.0);
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

        let segments = segment_game(rows.into_iter(), |r| r.idle_vils.parse::<u32>().ok());

        // Expected segments:
        // 1. [1000, 2000] value 3, Dark Age, AgeResearch
        // 2. [2000, 2000] value 3, Feudal Age, ValueChanged
        // 3. [2000, 3000] value 1, Feudal Age, EndOfData

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].age, "Dark Age");
        assert_eq!(segments[0].value, 3);
        assert_eq!(
            segments[0].end_reason,
            SegmentEndReason::AgeResearch("Feudal Age".to_string())
        );

        assert_eq!(segments[1].age, "Feudal Age");
        assert_eq!(segments[1].value, 3);
        assert_eq!(segments[1].start_ms, 2000);
        assert_eq!(segments[1].end_ms, 2000);
        assert_eq!(segments[1].end_reason, SegmentEndReason::ValueChanged);

        assert_eq!(segments[2].age, "Feudal Age");
        assert_eq!(segments[2].value, 1);
        assert_eq!(segments[2].start_ms, 2000);
        assert_eq!(segments[2].end_ms, 3000);
        assert_eq!(segments[2].end_reason, SegmentEndReason::EndOfData);
    }
}

pub mod mesher;
pub mod idle;
pub mod housing;
pub mod floating;
