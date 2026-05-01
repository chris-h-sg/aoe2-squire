use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct VsSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub idle_count: u32,
    pub age: String,
}

pub fn calculate_vs_lost(segments: &[VsSegment]) -> HashMap<String, f64> {
    let mut results = HashMap::new();
    for seg in segments {
        let duration_sec = (seg.end_ms - seg.start_ms) as f64 / 1000.0;
        let vs_lost = duration_sec * seg.idle_count as f64;
        *results.entry(seg.age.clone()).or_insert(0.0) += vs_lost;
    }
    results
}

pub fn segment_observations(records: impl Iterator<Item = csv::StringRecord>) -> Vec<VsSegment> {
    let mut segments = Vec::new();
    let mut current_idle_count: Option<u32> = None;
    let mut segment_start_ms: u64 = 0;
    let mut current_age = "Dark Age".to_string();

    for record in records {
        let ig_ms: u64 = record[0].parse().unwrap_or(0);
        let obs_type = &record[1];

        if obs_type == "ScreenGrab" {
            let idle_vils_str = &record[10];
            if !idle_vils_str.is_empty() {
                let new_idle_count: u32 = idle_vils_str.parse().unwrap_or(0);
                match current_idle_count {
                    Some(old_count) if new_idle_count != old_count => {
                        segments.push(VsSegment {
                            start_ms: segment_start_ms,
                            end_ms: ig_ms,
                            idle_count: old_count,
                            age: current_age.clone(),
                        });
                        segment_start_ms = ig_ms;
                        current_idle_count = Some(new_idle_count);
                    }
                    None => {
                        segment_start_ms = ig_ms;
                        current_idle_count = Some(new_idle_count);
                    }
                    _ => {}
                }
            }
        } else if obs_type == "RecEvent" {
            let desc = &record[2];
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
                if let Some(count) = current_idle_count {
                    segments.push(VsSegment {
                        start_ms: segment_start_ms,
                        end_ms: ig_ms,
                        idle_count: count,
                        age: current_age.clone(),
                    });
                }
                current_age = age_name.to_string();
                segment_start_ms = ig_ms;
            }
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vs_calculation() {
        let segments = vec![
            VsSegment {
                start_ms: 0,
                end_ms: 10000,
                idle_count: 3,
                age: "Dark Age".to_string(),
            },
            VsSegment {
                start_ms: 10000,
                end_ms: 20000,
                idle_count: 1,
                age: "Dark Age".to_string(),
            },
            VsSegment {
                start_ms: 20000,
                end_ms: 30000,
                idle_count: 5,
                age: "Feudal Age".to_string(),
            },
        ];

        let results = calculate_vs_lost(&segments);
        // Dark Age: (10s * 3) + (10s * 1) = 40 VS
        // Feudal Age: 10s * 5 = 50 VS
        assert_eq!(*results.get("Dark Age").unwrap(), 40.0);
        assert_eq!(*results.get("Feudal Age").unwrap(), 50.0);
    }

    #[test]
    fn test_segmentation() {
        let records = vec![
            csv::StringRecord::from(vec![
                "1000",
                "ScreenGrab",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "3",
            ]),
            csv::StringRecord::from(vec![
                "2000",
                "ScreenGrab",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "3",
            ]),
            csv::StringRecord::from(vec![
                "3000",
                "RecEvent",
                "Research Feudal Age",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
            ]),
            csv::StringRecord::from(vec![
                "4000",
                "ScreenGrab",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "1",
            ]),
        ];

        let segments = segment_observations(records.into_iter());
        assert_eq!(segments.len(), 2);

        // Segment 1: Start 1000, End 3000 (Feudal Click), Idle 3, Dark Age
        assert_eq!(segments[0].start_ms, 1000);
        assert_eq!(segments[0].end_ms, 3000);
        assert_eq!(segments[0].idle_count, 3);
        assert_eq!(segments[0].age, "Dark Age");

        // Segment 2: Start 3000, End 4000 (Count change), Idle 3, Feudal Age
        assert_eq!(segments[1].start_ms, 3000);
        assert_eq!(segments[1].end_ms, 4000);
        assert_eq!(segments[1].idle_count, 3);
        assert_eq!(segments[1].age, "Feudal Age");
    }
}
