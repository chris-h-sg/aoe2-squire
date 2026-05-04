use crate::types::Results;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub timestamp_ms: u64,
    pub frame_idx: u64,
    pub pop_color: String, // "white", "yellow", "overlay"
    pub pop_curr: u32,
    pub pop_max: u32,
    pub row_data: Results,
}

impl CapturedFrame {
    pub fn pop_diff(&self) -> i32 {
        (self.pop_max as i32) - (self.pop_curr as i32)
    }
}

pub struct InterpolationEngine {
    housed_timeout_ms: u64,
    queued_timeout_ms: u64,
    pending_buffer: VecDeque<CapturedFrame>,
    last_housed_anchor: Option<CapturedFrame>,
    last_queued_anchor: Option<CapturedFrame>,
}

impl Default for InterpolationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InterpolationEngine {
    pub fn new() -> Self {
        Self {
            housed_timeout_ms: crate::constants::HOUSED_INTERPOLATION_TIMEOUT_MS,
            queued_timeout_ms: crate::constants::QUEUED_INTERPOLATION_TIMEOUT_MS,
            pending_buffer: VecDeque::new(),
            last_housed_anchor: None,
            last_queued_anchor: None,
        }
    }

    pub fn process_frame(&mut self, mut frame: CapturedFrame) -> Vec<(Results, u64)> {
        let is_overlay = frame.pop_color == "overlay";
        let is_yellow = frame.pop_color == "yellow";
        let is_anchor = is_overlay || is_yellow;

        let mut output_rows = Vec::new();

        // Initialize status as "normal"
        set_housing_status(&mut frame.row_data, "normal");

        if is_anchor {
            // 1. Resolve bridges for buffered frames

            // Housed Bridge: overlay -> ... -> overlay
            if is_overlay {
                if let Some(ref last) = self.last_housed_anchor {
                    if frame.timestamp_ms - last.timestamp_ms <= self.housed_timeout_ms {
                        let max_diff = last.pop_diff().max(frame.pop_diff());
                        for bf in self.pending_buffer.iter_mut() {
                            if bf.pop_diff() <= max_diff {
                                set_housing_status(&mut bf.row_data, "housed");
                            }
                        }
                    }
                }
            }

            // Queued Bridge: (yellow|overlay) -> ... -> (yellow|overlay)
            if let Some(ref last) = self.last_queued_anchor {
                if frame.timestamp_ms - last.timestamp_ms <= self.queued_timeout_ms {
                    let max_diff = last.pop_diff().max(frame.pop_diff());
                    for bf in self.pending_buffer.iter_mut() {
                        // Only upgrade to queued if not already upgraded to housed
                        if get_housing_status(&bf.row_data) == "normal" && bf.pop_diff() <= max_diff
                        {
                            set_housing_status(&mut bf.row_data, "queued");
                        }
                    }
                }
            }

            // 2. Decide whether to flush or continue buffering
            if is_overlay {
                // Resolve all buffered frames and flush
                while let Some(bf) = self.pending_buffer.pop_front() {
                    output_rows.push((bf.row_data, bf.timestamp_ms));
                }

                set_housing_status(&mut frame.row_data, "housed");
                output_rows.push((frame.row_data.clone(), frame.timestamp_ms));

                self.last_housed_anchor = Some(frame.clone());
                self.last_queued_anchor = Some(frame);
            } else {
                // yellow
                // If a housed bridge is potentially still open, buffer this yellow frame
                let housed_active = if let Some(ref last) = self.last_housed_anchor {
                    frame.timestamp_ms - last.timestamp_ms <= self.housed_timeout_ms
                } else {
                    false
                };

                if housed_active {
                    set_housing_status(&mut frame.row_data, "queued");
                    self.pending_buffer.push_back(frame.clone());
                    self.last_queued_anchor = Some(frame);
                } else {
                    // No active housed bridge, resolve up to this yellow anchor and flush
                    while let Some(bf) = self.pending_buffer.pop_front() {
                        output_rows.push((bf.row_data, bf.timestamp_ms));
                    }

                    set_housing_status(&mut frame.row_data, "queued");
                    output_rows.push((frame.row_data.clone(), frame.timestamp_ms));

                    self.last_housed_anchor = None;
                    self.last_queued_anchor = Some(frame);
                }
            }
        } else {
            // Non-anchor frame (white)
            let housed_active = if let Some(ref last) = self.last_housed_anchor {
                frame.timestamp_ms - last.timestamp_ms <= self.housed_timeout_ms
            } else {
                false
            };
            let queued_active = if let Some(ref last) = self.last_queued_anchor {
                frame.timestamp_ms - last.timestamp_ms <= self.queued_timeout_ms
            } else {
                false
            };

            if housed_active || queued_active {
                self.pending_buffer.push_back(frame);
            } else {
                // Break: flush buffer and current frame
                while let Some(bf) = self.pending_buffer.pop_front() {
                    output_rows.push((bf.row_data, bf.timestamp_ms));
                }
                output_rows.push((frame.row_data, frame.timestamp_ms));

                self.last_housed_anchor = None;
                self.last_queued_anchor = None;
            }
        }

        output_rows
    }

    pub fn flush(&mut self) -> Vec<(Results, u64)> {
        let mut output_rows = Vec::new();
        while let Some(bf) = self.pending_buffer.pop_front() {
            output_rows.push((bf.row_data, bf.timestamp_ms));
        }
        output_rows
    }
}

fn set_housing_status(results: &mut Results, status: &str) {
    results
        .entry("population".to_string())
        .or_default()
        .insert("status".to_string(), status.to_string());
}

fn get_housing_status(results: &Results) -> &str {
    results
        .get("population")
        .and_then(|m| m.get("status"))
        .map(|s| s.as_str())
        .unwrap_or("normal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv::ReaderBuilder;
    use std::collections::HashMap;
    use std::path::Path;

    fn run_scenario(name: &str) {
        let test_data_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_bench")
            .join("interpolation")
            .join(format!("{}.csv", name));

        println!("Running scenario: {}", test_data_path.display());

        let mut rdr = ReaderBuilder::new()
            .from_path(&test_data_path)
            .expect("Failed to open test CSV");

        let mut engine = InterpolationEngine::new();
        let mut expected_housing = Vec::new();
        let mut actual_rows = Vec::new();

        for result in rdr.records() {
            let record = result.expect("Failed to read CSV record");

            let ts: f64 = record[0].parse().unwrap();
            let frame_idx: u64 = record[1].parse().unwrap();
            let pop_total: u32 = record[2].parse().unwrap();
            let pop_max: u32 = record[3].parse().unwrap();
            let pop_color = &record[4];
            let expected = &record[5];

            expected_housing.push(expected.to_string());

            let mut results = Results::new();
            let mut pop = HashMap::new();
            pop.insert("total".to_string(), format!("{}/{}", pop_total, pop_max));
            pop.insert("color".to_string(), pop_color.to_string());
            results.insert("population".to_string(), pop);

            let frame = CapturedFrame {
                timestamp_ms: (ts * 1000.0) as u64,
                frame_idx,
                pop_color: pop_color.to_string(),
                pop_curr: pop_total,
                pop_max,
                row_data: results,
            };

            actual_rows.extend(engine.process_frame(frame));
        }

        actual_rows.extend(engine.flush());

        assert_eq!(
            actual_rows.len(),
            expected_housing.len(),
            "Row count mismatch in scenario {}",
            name
        );

        for (i, (actual, expected)) in actual_rows.iter().zip(expected_housing.iter()).enumerate() {
            let actual_status = get_housing_status(&actual.0);
            assert_eq!(
                actual_status, expected,
                "Mismatch at row {} in scenario {}",
                i, name
            );
        }
    }

    #[test]
    fn test_scenario_flicker() {
        run_scenario("flicker");
    }

    #[test]
    fn test_scenario_housed_fallback_to_queued() {
        run_scenario("housed_fallback_to_queued");
    }

    #[test]
    fn test_scenario_house_completion() {
        run_scenario("house_completion");
    }

    #[test]
    fn test_scenario_mixed_priority() {
        run_scenario("mixed_priority");
    }

    #[test]
    fn test_scenario_priority_overlap() {
        run_scenario("priority_overlap");
    }

    #[test]
    fn test_scenario_queued_flicker() {
        run_scenario("queued_flicker");
    }

    #[test]
    fn test_scenario_queued_timeout() {
        run_scenario("queued_timeout");
    }

    #[test]
    fn test_scenario_state_transition() {
        run_scenario("state_transition");
    }

    #[test]
    fn test_scenario_timeout() {
        run_scenario("timeout");
    }

    #[test]
    fn test_scenario_unit_death() {
        run_scenario("unit_death");
    }
}
