/// Smooths out sudden outliers in a numeric sequence.
/// 
/// A spike is defined as a sequence of frames where the values deviate from 
/// the surrounding trend by more than `min_spike` and then return to the trend.
/// Interpolates values for spikes up to `max_duration`.
pub fn smooth_sequence(values: &mut [Option<u32>], min_spike: i32, max_duration: usize) -> usize {
    let n = values.len();
    let mut i = 0;
    let mut smoothed_count = 0;

    while i < n {
        let start_val = match values[i] {
            Some(v) => v as f64,
            None => {
                i += 1;
                continue;
            }
        };

        for duration in 1..=max_duration {
            let j = i + duration + 1;
            if j >= n {
                continue;
            }

            let end_val = match values[j] {
                Some(v) => v as f64,
                None => continue,
            };

            // CRITICAL: The end point must be "near" the start point (on trend)
            // so we don't accidentally smooth out legitimate step changes.
            if (end_val - start_val).abs() >= min_spike as f64 {
                continue;
            }

            let mut all_spikes = true;
            let mut direction = 0;

            for k in i + 1..j {
                let t = (k - i) as f64 / (j - i) as f64;
                let target = start_val + (end_val - start_val) * t;

                match values[k] {
                    None => {}
                    Some(v) => {
                        let diff = v as f64 - target;
                        if diff.abs() < min_spike as f64 {
                            all_spikes = false;
                            break;
                        }

                        let current_dir = if diff > 0.0 { 1 } else { -1 };
                        if direction == 0 {
                            direction = current_dir;
                        } else if direction != current_dir {
                            all_spikes = false;
                            break;
                        }
                    }
                }
            }

            if all_spikes {
                for k in i + 1..j {
                    let t = (k - i) as f64 / (j - i) as f64;
                    let target = (start_val + (end_val - start_val) * t).round() as u32;
                    values[k] = Some(target);
                    smoothed_count += 1;
                }
                i = j - 1;
                break;
            }
        }
        i += 1;
    }
    smoothed_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoothing_single_spike() {
        let mut data = vec![Some(10), Some(70), Some(10)];
        let count = smooth_sequence(&mut data, 5, 1);
        assert_eq!(count, 1);
        assert_eq!(data[1], Some(10));
    }

    #[test]
    fn test_smoothing_multi_frame_spike() {
        let mut data = vec![Some(10), Some(70), Some(75), Some(10)];
        let count = smooth_sequence(&mut data, 5, 2);
        assert_eq!(count, 2);
        assert_eq!(data[1], Some(10));
        assert_eq!(data[2], Some(10));
    }

    #[test]
    fn test_smoothing_step_change_not_smoothed() {
        let mut data = vec![Some(10), Some(20), Some(20), Some(20)];
        let count = smooth_sequence(&mut data, 5, 2);
        assert_eq!(count, 0);
        assert_eq!(data[1], Some(20));
    }

    #[test]
    fn test_smoothing_none_handling() {
        let mut data = vec![Some(10), None, Some(10)];
        let count = smooth_sequence(&mut data, 5, 1);
        assert_eq!(count, 1);
        assert_eq!(data[1], Some(10));
    }

    #[test]
    fn test_smoothing_interpolation_on_trend() {
        let mut data = vec![Some(10), Some(70), Some(12)];
        let count = smooth_sequence(&mut data, 5, 1);
        assert_eq!(count, 1);
        assert_eq!(data[1], Some(11)); // (10+12)/2
    }
}
