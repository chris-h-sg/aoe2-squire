//! Replay/telemetry sync: the mapping between real-world time (telemetry CSV) and in-game
//! time (replay), built by matching replay spends to resource drops on screen over the
//! whole game. It copes with any game speed, pauses and a speed that changes (replay
//! playback), and returns an error instead of a guess when the two do not line up.
//! See DECISION_LOG.md ("Replay/Telemetry Sync").

mod align;
pub mod mapping;

pub use mapping::{Knot, Pause, TimeMapping, TimePair};

use crate::replay::ReplayEvent;
use crate::types::CsvRow;
use serde::Serialize;
use std::fmt;

/// Game speeds searched (in-game ms per real-world ms): Slow is 1.0, Normal 1.7, Fast 2.0;
/// replay playback can run faster.
pub const MIN_SPEED: f64 = 0.5;
pub const MAX_SPEED: f64 = 4.0;
/// Final matches must be this close to the fitted mapping on top of their own uncertainty
/// (real-world ms); looser than a segment, so a late frame still counts as matched.
const MATCH_SLACK_MS: f64 = 275.0;
/// Fewer matched spends than this, or a smaller share, is not a calibration.
const MIN_MATCHED: usize = 10;
const MIN_MATCHED_SHARE: f64 = 0.5;

/// Where a time mapping came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MappingSource {
    /// Replay spends matched to resource drops on screen.
    SpendAlignment,
    // Phase 2: GameClock, from reading the in-game clock (F11).
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncQuality {
    /// Replay spends that fall inside the captured time range.
    pub spends: usize,
    /// Of those, spends matched to a drop on screen.
    pub matched: usize,
    pub matched_share: f64,
    /// Distance of the matched drops from the mapping (real-world ms).
    pub residual_median_ms: f64,
    pub residual_p90_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Calibration {
    pub mapping: TimeMapping,
    pub source: MappingSource,
    pub quality: SyncQuality,
    /// The matched (spend, drop) times the mapping was fitted to.
    #[serde(skip)]
    pub matches: Vec<TimePair>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncError {
    NoSpends,
    NoDrops,
    /// No stretch of the game where spends and drops line up.
    NoAlignment {
        spends: usize,
        drops: usize,
    },
    TooFewMatches {
        matched: usize,
        spends: usize,
    },
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SyncError::NoSpends => {
                write!(f, "sync: the replay has no spends for the recorded player")
            }
            SyncError::NoDrops => write!(f, "sync: no resource drop readable on screen"),
            SyncError::NoAlignment { spends, drops } => write!(
                f,
                "sync: {} replay spends and {} drops on screen never line up (wrong replay?)",
                spends, drops
            ),
            SyncError::TooFewMatches { matched, spends } => write!(
                f,
                "sync: only {} of {} replay spends match a drop on screen (need {} and {:.0}%)",
                matched,
                spends,
                MIN_MATCHED,
                MIN_MATCHED_SHARE * 100.0
            ),
        }
    }
}

impl std::error::Error for SyncError {}

/// Builds the time mapping from telemetry rows (sorted by time) and the recorded player's
/// replay events.
pub fn calibrate(rows: &[CsvRow], events: &[ReplayEvent]) -> Result<Calibration, SyncError> {
    let spends = align::spends(events);
    if spends.is_empty() {
        return Err(SyncError::NoSpends);
    }
    let drops = align::drops(rows);
    if drops.is_empty() {
        return Err(SyncError::NoDrops);
    }
    let aligner = align::Aligner::new(&spends, &drops);
    let no_alignment = || SyncError::NoAlignment {
        spends: spends.len(),
        drops: drops.len(),
    };

    let mut mapping =
        mapping::fit_mapping(&aligner.local_pairs(), rows).ok_or_else(no_alignment)?;
    // Match against the mapping, refit on those matches, and match once more.
    let mut matches = aligner.matches(&mapping, MATCH_SLACK_MS);
    mapping = mapping::fit_mapping(&matches, rows).ok_or_else(no_alignment)?;
    matches = aligner.matches(&mapping, MATCH_SLACK_MS);

    let (first_rw, last_rw) = (
        rows[0].timestamp_ms as f64,
        rows[rows.len() - 1].timestamp_ms as f64,
    );
    let captured = spends
        .iter()
        .filter(|s| (first_rw..=last_rw).contains(&mapping.rw_at(s.ig_ms)))
        .count();
    let matched = matches
        .iter()
        .filter(|p| (first_rw..=last_rw).contains(&mapping.rw_at(p.ig_ms)))
        .count();
    let mut residuals: Vec<f64> = matches
        .iter()
        .map(|p| (p.rw_ms - mapping.rw_at(p.ig_ms)).abs())
        .collect();
    residuals.sort_by(f64::total_cmp);
    let pct = |p: usize| {
        residuals
            .get((residuals.len().saturating_sub(1)) * p / 100)
            .copied()
            .unwrap_or(0.0)
    };
    let quality = SyncQuality {
        spends: captured,
        matched,
        matched_share: matched as f64 / captured.max(1) as f64,
        residual_median_ms: pct(50),
        residual_p90_ms: pct(90),
    };
    if matched < MIN_MATCHED || quality.matched_share < MIN_MATCHED_SHARE {
        return Err(SyncError::TooFewMatches {
            matched,
            spends: captured,
        });
    }
    Ok(Calibration {
        mapping,
        source: MappingSource::SpendAlignment,
        quality,
        matches,
    })
}

impl Calibration {
    /// One line for logs, e.g. "speed 1.690, 2 segments, 1 pause (8.0 s), 188/189 spends
    /// matched (99.5%), residual median 70 ms".
    pub fn summary(&self) -> String {
        let m = &self.mapping;
        let pauses = m.pauses();
        let first = m.knots()[0].ig_ms;
        let last = m.knots()[m.knots().len() - 1].ig_ms;
        let speeds: Vec<f64> = [first, (first + last) / 2.0, last]
            .iter()
            .map(|&t| m.speed_at(t))
            .collect();
        let speed = if speeds.iter().all(|s| (s - speeds[0]).abs() < 0.01) {
            format!("speed {:.3}", speeds[0])
        } else {
            format!(
                "speed {:.3}..{:.3}",
                speeds.iter().cloned().fold(f64::INFINITY, f64::min),
                speeds.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            )
        };
        let pause_text = if pauses.is_empty() {
            "no pause".to_string()
        } else {
            let lengths: Vec<String> = pauses
                .iter()
                .map(|p| format!("{:.1} s", p.duration_ms / 1000.0))
                .collect();
            format!("{} pause(s) ({})", pauses.len(), lengths.join(", "))
        };
        let q = &self.quality;
        format!(
            "{}, {} segment(s), {}, {}/{} spends matched ({:.1}%), residual median {:.0} ms",
            speed,
            m.segments(),
            pause_text,
            q.matched,
            q.spends,
            q.matched_share * 100.0,
            q.residual_median_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::ResourceCost;

    fn row(ts: u64, food: u32, wood: u32) -> CsvRow {
        CsvRow {
            timestamp_ms: ts,
            food: Some(food),
            wood: Some(wood),
            gold: Some(100),
            stone: Some(200),
            pop_curr: None,
            pop_max: None,
            pop_vils: None,
            idle_vils: None,
            housing: None,
        }
    }

    fn queue(ig_ms: u32, food: u32, wood: u32) -> ReplayEvent {
        ReplayEvent::UnitQueued {
            timestamp_ms: ig_ms,
            player_id: 1,
            unit_type: "Unit".to_string(),
            cost: ResourceCost {
                food,
                wood,
                gold: 0,
                stone: 0,
            },
            building_ids: vec![1],
            building_types: vec!["Town Center".to_string()],
        }
    }

    /// A synthetic game: spends at irregular times and costs, shown on screen at real-world
    /// time `rw_of(ig)`; resources are topped up between spends so they never run out.
    fn game(rw_of: impl Fn(f64) -> f64) -> (Vec<CsvRow>, Vec<ReplayEvent>) {
        let costs = [
            (50, 0),
            (0, 25),
            (50, 0),
            (0, 100),
            (60, 0),
            (50, 0),
            (0, 175),
        ];
        let events: Vec<ReplayEvent> = (0..80u32)
            .map(|i| {
                let (f, w) = costs[(i as usize * 3 + i as usize / 5) % costs.len()];
                queue(2_000 + i * 9_000 + (i * i * 37) % 4_000, f, w)
            })
            .collect();
        let end_rw = rw_of(800_000.0) as u64;
        let (mut food, mut wood) = (5_000u32, 5_000u32);
        let mut next = 0;
        let rows = (0..=end_rw / 250)
            .map(|k| {
                let ts = 10_000 + k * 250;
                while next < events.len()
                    && rw_of(events[next].timestamp_ms() as f64) + 10_000.0 <= ts as f64
                {
                    let c = events[next].cost().unwrap();
                    food -= c.food;
                    wood -= c.wood;
                    next += 1;
                }
                row(ts, food, wood)
            })
            .collect();
        (rows, events)
    }

    #[test]
    fn recovers_constant_speed() {
        for speed in [1.0, 1.5, 1.7, 2.0, 3.4] {
            let (rows, events) = game(|ig| ig / speed);
            let cal = calibrate(&rows, &events).unwrap();
            let start = cal.mapping.game_start_rw_ms();
            assert!(
                (start - 10_000.0).abs() < 300.0,
                "speed {}: start {}",
                speed,
                start
            );
            assert!((cal.mapping.speed_at(300_000.0) - speed).abs() < 0.01 * speed);
            assert!(cal.quality.matched_share > 0.95, "{}", cal.summary());
        }
    }

    #[test]
    fn rejects_unrelated_replay() {
        let (rows, _) = game(|ig| ig / 1.7);
        let events: Vec<ReplayEvent> = (0..80).map(|i| queue(1_000 + i * 7_000, 0, 0)).collect();
        assert_eq!(calibrate(&rows, &events), Err(SyncError::NoSpends));
        let events: Vec<ReplayEvent> = (0..80)
            .map(|i| queue(1_000 + i * 7_000, 0, 1_000))
            .collect();
        assert!(calibrate(&rows, &events).is_err());
    }
}
