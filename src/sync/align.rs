//! Matching replay spends to resource drops on screen, without knowing the game speed or
//! where the game was paused.
//!
//! 1. Drops: every frame where a resource reads lower than its previous reading. Unread
//!    values are skipped (never a drop) and single-frame misreads are ignored.
//! 2. Local alignment: the replay timeline is cut into overlapping windows. In each, every
//!    (spend, compatible drop) pair votes for the speed and offset that would put them
//!    together; the strongest vote is refined by a least-squares fit. Rare spends (an
//!    Age-up, a Blacksmith) weigh more than villagers, whose regular rhythm would otherwise
//!    also line up one villager off.
//! 3. Each spend takes its drop from the best window covering it; `mapping::fit_mapping`
//!    turns those pairs into the time mapping.
//! 4. With the mapping, every spend is matched once more, each drop paying for at most
//!    what it shows, and the mapping is fitted again on those matches.

use super::mapping::{TimeMapping, TimePair};
use super::{MAX_SPEED, MIN_SPEED};
use crate::replay::ReplayEvent;
use crate::types::CsvRow;
use std::collections::HashMap;

/// Civ discounts and gathering within the same frame make the drop smaller than the cost.
const MIN_DROP_FRACTION: f64 = 0.4;
/// A drop after readings this far apart cannot be timed well enough (real-world ms).
const MAX_DROP_GAP_MS: u64 = 1_500;
/// Only closer matches vote in the local alignment.
const VOTE_MIN_SIMILARITY: f64 = 0.5;
const WINDOW_IG_MS: f64 = 120_000.0;
const WINDOW_STEP_IG_MS: f64 = 60_000.0;
/// Ratio between neighbouring speeds in the search grid.
const SPEED_STEP: f64 = 1.005;
/// Offset histogram bin; a vote counts in a bin and its two neighbours (real-world ms).
const OFFSET_BIN_MS: f64 = 250.0;
/// How far a drop may be from where a window's alignment expects it, on top of its own
/// uncertainty (real-world ms).
const WINDOW_SLACK_MS: f64 = 625.0;
const MIN_WINDOW_INLIERS: usize = 3;

/// A resource drop seen on screen.
#[derive(Debug, Clone)]
pub(super) struct Drop {
    /// Best estimate of when the spend happened: halfway between the frame showing the
    /// drop and the reading before it.
    pub event_rw_ms: f64,
    /// Half the time between those two readings.
    pub uncertainty_ms: f64,
    pub delta: [u32; 4],
}

/// A replay command that costs resources.
#[derive(Debug, Clone)]
pub(super) struct Spend {
    pub ig_ms: f64,
    pub cost: [u32; 4],
}

fn resource(row: &CsvRow, r: usize) -> Option<u32> {
    [row.food, row.wood, row.gold, row.stone][r]
}

pub(super) fn drops(rows: &[CsvRow]) -> Vec<Drop> {
    // Per row: (latest timestamp of the reading before the drop, drop per resource).
    let mut per_row: Vec<Option<(u64, [u32; 4])>> = vec![None; rows.len()];
    for r in 0..4 {
        let series: Vec<(usize, u64, u32)> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| resource(row, r).map(|v| (i, row.timestamp_ms, v)))
            .collect();
        // A-B-A: B is a misread, so neither A->B nor B->A is a real change.
        let glitch = |k: usize| {
            k > 0 && k + 1 < series.len() && series[k - 1].2 == series[k + 1].2 && {
                series[k].2 != series[k - 1].2
            }
        };
        for k in 1..series.len() {
            let (_, ts0, v0) = series[k - 1];
            let (i, ts1, v1) = series[k];
            if v1 < v0 && ts1 - ts0 <= MAX_DROP_GAP_MS && !glitch(k) && !glitch(k - 1) {
                let entry = per_row[i].get_or_insert((ts0, [0; 4]));
                entry.0 = entry.0.max(ts0);
                entry.1[r] = v0 - v1;
            }
        }
    }
    rows.iter()
        .zip(per_row)
        .filter_map(|(row, d)| {
            d.map(|(before, delta)| Drop {
                event_rw_ms: (before + row.timestamp_ms) as f64 / 2.0,
                uncertainty_ms: (row.timestamp_ms - before) as f64 / 2.0,
                delta,
            })
        })
        .collect()
}

pub(super) fn spends(events: &[ReplayEvent]) -> Vec<Spend> {
    let mut out: Vec<Spend> = events
        .iter()
        .filter_map(|e| {
            let c = e.cost()?;
            let cost = [c.food, c.wood, c.gold, c.stone];
            cost.iter().any(|&v| v > 0).then_some(Spend {
                ig_ms: e.timestamp_ms() as f64,
                cost,
            })
        })
        .collect();
    out.sort_by(|a, b| a.ig_ms.total_cmp(&b.ig_ms));
    out
}

/// How well a drop explains a spend (1 = exactly its cost); None if it cannot be the spend.
fn similarity(cost: &[u32; 4], delta: &[u32; 4]) -> Option<f64> {
    let (mut num, mut den) = (0.0, 0.0);
    for r in 0..4 {
        if cost[r] == 0 {
            continue;
        }
        let (c, d) = (cost[r] as f64, delta[r] as f64);
        if d < MIN_DROP_FRACTION * c {
            return None;
        }
        num += c * d.min(c) / d.max(c);
        den += c;
    }
    Some(num / den)
}

/// Whether what is left of a drop can still be (a discounted) `cost`.
fn affordable(cost: &[u32; 4], left: &[u32; 4]) -> bool {
    (0..4).all(|r| cost[r] == 0 || left[r] as f64 >= MIN_DROP_FRACTION * cost[r] as f64)
}

fn pay(cost: &[u32; 4], left: &mut [u32; 4]) {
    for r in 0..4 {
        left[r] -= left[r].min(cost[r]);
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    drop: usize,
    rw_ms: f64,
    uncertainty_ms: f64,
    sim: f64,
}

impl Candidate {
    /// Whether the drop is within `slack` plus its own uncertainty of `expected_rw`.
    fn near(&self, expected_rw: f64, slack: f64) -> bool {
        (self.rw_ms - expected_rw).abs() <= slack + self.uncertainty_ms
    }

    fn pair(&self, ig_ms: f64) -> TimePair {
        TimePair {
            ig_ms,
            rw_ms: self.rw_ms,
            uncertainty_ms: self.uncertainty_ms,
        }
    }
}

/// Orders candidates by similarity, then by closeness to the expected time.
fn closer_match(x: &Candidate, y: &Candidate, expected_rw: f64) -> std::cmp::Ordering {
    x.sim.total_cmp(&y.sim).then(
        (y.rw_ms - expected_rw)
            .abs()
            .total_cmp(&(x.rw_ms - expected_rw).abs()),
    )
}

/// A straight alignment for one window of the replay: rw = a + b * ig.
#[derive(Debug, Clone)]
struct WindowFit {
    spends: std::ops::Range<usize>,
    a: f64,
    b: f64,
    score: f64,
}

pub(super) struct Aligner<'a> {
    spends: &'a [Spend],
    drops: &'a [Drop],
    /// Per spend, every drop that could be it.
    candidates: Vec<Vec<Candidate>>,
    /// Per spend: 1 / sqrt(number of spends with the same cost).
    weight: Vec<f64>,
}

impl<'a> Aligner<'a> {
    pub fn new(spends: &'a [Spend], drops: &'a [Drop]) -> Aligner<'a> {
        let candidates = spends
            .iter()
            .map(|s| {
                drops
                    .iter()
                    .enumerate()
                    .filter_map(|(k, d)| {
                        similarity(&s.cost, &d.delta).map(|sim| Candidate {
                            drop: k,
                            rw_ms: d.event_rw_ms,
                            uncertainty_ms: d.uncertainty_ms,
                            sim,
                        })
                    })
                    .collect()
            })
            .collect();
        let weight = spends
            .iter()
            .map(|s| {
                let same = spends.iter().filter(|o| o.cost == s.cost).count();
                1.0 / (same as f64).sqrt()
            })
            .collect();
        Aligner {
            spends,
            drops,
            candidates,
            weight,
        }
    }

    /// One pair per spend from the local alignments (step 3), not yet checked against each
    /// other: wrong matches are left for the mapping fit to discard.
    pub fn local_pairs(&self) -> Vec<TimePair> {
        let windows = self.window_fits();
        (0..self.spends.len())
            .filter_map(|i| {
                let mut covering: Vec<&WindowFit> =
                    windows.iter().filter(|w| w.spends.contains(&i)).collect();
                covering.sort_by(|x, y| y.score.total_cmp(&x.score));
                covering.iter().find_map(|w| {
                    let t = self.spends[i].ig_ms;
                    self.best_candidate(i, w.a + w.b * t, WINDOW_SLACK_MS)
                        .map(|c| c.pair(t))
                })
            })
            .collect()
    }

    /// Matches every spend to a drop near where `mapping` puts it (step 4). A drop pays for
    /// at most what it shows, so two spends in one frame share it. `slack` is added to each
    /// drop's own uncertainty.
    pub fn matches(&self, mapping: &TimeMapping, slack: f64) -> Vec<TimePair> {
        let mut left: Vec<[u32; 4]> = self.drops.iter().map(|d| d.delta).collect();
        self.spends
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let expected = mapping.rw_at(s.ig_ms);
                let c = self.candidates[i]
                    .iter()
                    .filter(|c| c.near(expected, slack))
                    .filter(|c| affordable(&s.cost, &left[c.drop]))
                    .min_by(|x, y| {
                        (x.rw_ms - expected)
                            .abs()
                            .total_cmp(&(y.rw_ms - expected).abs())
                    })?;
                pay(&s.cost, &mut left[c.drop]);
                Some(c.pair(s.ig_ms))
            })
            .collect()
    }

    /// The closest match to the expected time, among the best matching drops.
    fn best_candidate(&self, spend: usize, expected_rw: f64, slack: f64) -> Option<Candidate> {
        self.candidates[spend]
            .iter()
            .filter(|c| c.near(expected_rw, slack))
            .max_by(|x, y| closer_match(x, y, expected_rw))
            .copied()
    }

    fn window_fits(&self) -> Vec<WindowFit> {
        let Some(last) = self.spends.last() else {
            return vec![];
        };
        let first_ig = self.spends[0].ig_ms;
        let mut fits = vec![];
        let mut start = first_ig;
        loop {
            let lo = self.spends.partition_point(|s| s.ig_ms < start);
            let hi = self
                .spends
                .partition_point(|s| s.ig_ms < start + WINDOW_IG_MS);
            if let Some(fit) = self.fit_window(lo..hi) {
                fits.push(fit);
            }
            if start + WINDOW_IG_MS > last.ig_ms {
                break;
            }
            start += WINDOW_STEP_IG_MS;
        }
        fits
    }

    fn fit_window(&self, spends: std::ops::Range<usize>) -> Option<WindowFit> {
        // (in-game ms, real-world ms, weight) per vote.
        let votes: Vec<(f64, f64, f64)> = spends
            .clone()
            .flat_map(|i| {
                self.candidates[i]
                    .iter()
                    .filter(|c| c.sim >= VOTE_MIN_SIMILARITY)
                    .map(move |c| (self.spends[i].ig_ms, c.rw_ms, c.sim * self.weight[i]))
            })
            .collect();
        let voting_spends = spends
            .clone()
            .filter(|&i| {
                self.candidates[i]
                    .iter()
                    .any(|c| c.sim >= VOTE_MIN_SIMILARITY)
            })
            .count();
        if voting_spends < MIN_WINDOW_INLIERS {
            return None;
        }

        let t_min = self.spends[spends.start].ig_ms;
        let t_max = self.spends[spends.end - 1].ig_ms;
        let r_min = votes.iter().map(|v| v.1).fold(f64::INFINITY, f64::min);
        let r_max = votes.iter().map(|v| v.1).fold(f64::NEG_INFINITY, f64::max);
        // Offsets (rw at in-game 0) range over [r_min - t_max / s, r_max - t_min / s].
        let bins = ((r_max - r_min + (t_max - t_min) / MIN_SPEED) / OFFSET_BIN_MS) as usize + 3;
        let mut hist = vec![0.0f64; bins];
        let mut touched: Vec<usize> = Vec::with_capacity(votes.len());
        let mut best: Option<(f64, f64, f64)> = None; // (score, speed, offset)
        let mut speed = MIN_SPEED;
        while speed <= MAX_SPEED {
            let o_min = r_min - t_max / speed;
            for &(t, r, w) in &votes {
                let k = ((r - t / speed - o_min) / OFFSET_BIN_MS) as usize + 1;
                hist[k] += w;
                touched.push(k);
            }
            for &k in &touched {
                let score = hist[k - 1] + hist[k] + hist[k + 1];
                if best.is_none_or(|b| score > b.0) {
                    let offset = o_min + (k as f64 - 0.5) * OFFSET_BIN_MS;
                    best = Some((score, speed, offset));
                }
            }
            for k in touched.drain(..) {
                hist[k] = 0.0;
            }
            speed *= SPEED_STEP;
        }
        let (_, speed, offset) = best?;
        self.refine(spends, offset, 1.0 / speed)
    }

    /// Least-squares fit on the matches near a voted alignment, tightened once.
    fn refine(&self, spends: std::ops::Range<usize>, a: f64, b: f64) -> Option<WindowFit> {
        let (mut a, mut b) = (a, b);
        let mut score = 0.0;
        for slack in [WINDOW_SLACK_MS, super::mapping::SEGMENT_SLACK_MS] {
            let mut inliers = vec![];
            score = 0.0;
            // A drop pays for at most what it shows, so a burst of spends cannot all claim
            // the same drop.
            let mut left: HashMap<usize, [u32; 4]> = HashMap::new();
            for i in spends.clone() {
                let s = &self.spends[i];
                let expected = a + b * s.ig_ms;
                let c = self.candidates[i]
                    .iter()
                    .filter(|c| c.near(expected, slack))
                    .filter(|c| {
                        affordable(
                            &s.cost,
                            left.get(&c.drop).unwrap_or(&self.drops[c.drop].delta),
                        )
                    })
                    .max_by(|x, y| closer_match(x, y, expected));
                if let Some(c) = c {
                    pay(
                        &s.cost,
                        left.entry(c.drop).or_insert(self.drops[c.drop].delta),
                    );
                    inliers.push((s.ig_ms, c.rw_ms));
                    score += c.sim * self.weight[i];
                }
            }
            if inliers.len() < MIN_WINDOW_INLIERS {
                return None;
            }
            let n = inliers.len() as f64;
            let mt = inliers.iter().map(|p| p.0).sum::<f64>() / n;
            let mr = inliers.iter().map(|p| p.1).sum::<f64>() / n;
            let stt: f64 = inliers.iter().map(|p| (p.0 - mt).powi(2)).sum();
            let str_: f64 = inliers.iter().map(|p| (p.0 - mt) * (p.1 - mr)).sum();
            if stt > 0.0 {
                let fitted = str_ / stt;
                if (1.0 / MAX_SPEED..=1.0 / MIN_SPEED).contains(&fitted) {
                    b = fitted;
                }
            }
            a = mr - b * mt;
        }
        Some(WindowFit {
            spends,
            a,
            b,
            score,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts: u64, food: Option<u32>, wood: Option<u32>) -> CsvRow {
        CsvRow {
            timestamp_ms: ts,
            food,
            wood,
            gold: Some(100),
            stone: Some(200),
            pop_curr: None,
            pop_max: None,
            pop_vils: None,
            idle_vils: None,
            housing: None,
        }
    }

    #[test]
    fn unread_values_and_misreads_are_not_drops() {
        let rows = vec![
            row(0, Some(200), Some(200)),
            row(250, None, Some(200)),
            row(500, Some(200), Some(200)),
            row(750, Some(200), Some(120)), // misread: back to 200 next frame
            row(1000, Some(208), Some(200)),
            row(1250, Some(150), Some(175)),
        ];
        let d = drops(&rows);
        assert_eq!(d.len(), 1, "{:?}", d);
        assert_eq!(d[0].delta, [58, 25, 0, 0]);
        assert_eq!(d[0].event_rw_ms, 1125.0);
        assert_eq!(d[0].uncertainty_ms, 125.0);
    }

    #[test]
    fn similarity_allows_discounts_but_not_tiny_drops() {
        assert_eq!(similarity(&[50, 0, 0, 0], &[50, 0, 0, 0]), Some(1.0));
        assert_eq!(similarity(&[50, 0, 0, 0], &[100, 30, 0, 0]), Some(0.5));
        assert_eq!(similarity(&[500, 0, 0, 0], &[50, 0, 0, 0]), None);
        assert_eq!(similarity(&[0, 25, 0, 0], &[50, 0, 0, 0]), None);
    }
}
