//! The real-world ↔ in-game time mapping, and fitting it to matched time pairs.
//!
//! The mapping is source-agnostic: today the pairs come from replay spends matched to
//! resource drops on screen (`super::align`); a reader of the in-game clock (F11) would
//! produce the same kind of pairs and reuse `fit_mapping`.

use super::{MAX_SPEED, MIN_SPEED};
use crate::types::CsvRow;
use serde::Serialize;

/// A point where the mapping bends: real-world `rw_ms` corresponds to in-game `ig_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Knot {
    pub rw_ms: f64,
    pub ig_ms: f64,
}

/// A stretch of real-world time during which the in-game clock stood still.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pause {
    pub ig_ms: f64,
    pub rw_start_ms: f64,
    pub duration_ms: f64,
}

/// Monotonic piecewise-linear mapping between real-world and in-game time. Between knots
/// time runs linearly (the slope is the game speed, in-game ms per real-world ms); two knots
/// with the same `ig_ms` are a pause. Outside the knots the first and last speed continue.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimeMapping {
    knots: Vec<Knot>,
    speed_before: f64,
    speed_after: f64,
}

impl TimeMapping {
    /// Constant speed from `start_rw_ms` (in-game time 0) on.
    pub fn linear(start_rw_ms: f64, speed: f64) -> TimeMapping {
        TimeMapping {
            knots: vec![Knot {
                rw_ms: start_rw_ms,
                ig_ms: 0.0,
            }],
            speed_before: speed,
            speed_after: speed,
        }
    }

    /// Builds a mapping from knots sorted by time; neither clock may run backwards.
    pub fn from_knots(
        knots: Vec<Knot>,
        speed_before: f64,
        speed_after: f64,
    ) -> Result<TimeMapping, String> {
        if knots.is_empty() {
            return Err("a time mapping needs at least one knot".to_string());
        }
        if !(speed_before > 0.0 && speed_after > 0.0) {
            return Err("a time mapping needs a positive speed at both ends".to_string());
        }
        if knots
            .windows(2)
            .any(|w| w[1].rw_ms < w[0].rw_ms || w[1].ig_ms < w[0].ig_ms)
        {
            return Err("time mapping knots must not run backwards".to_string());
        }
        Ok(TimeMapping {
            knots,
            speed_before,
            speed_after,
        })
    }

    pub fn knots(&self) -> &[Knot] {
        &self.knots
    }

    /// In-game time at real-world time `rw_ms` (negative before the game started).
    pub fn ig_at(&self, rw_ms: f64) -> f64 {
        let (first, last) = (self.knots[0], self.knots[self.knots.len() - 1]);
        if rw_ms <= first.rw_ms {
            return first.ig_ms - (first.rw_ms - rw_ms) * self.speed_before;
        }
        if rw_ms >= last.rw_ms {
            return last.ig_ms + (rw_ms - last.rw_ms) * self.speed_after;
        }
        let i = self.knots.partition_point(|k| k.rw_ms <= rw_ms);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        a.ig_ms + (b.ig_ms - a.ig_ms) * (rw_ms - a.rw_ms) / (b.rw_ms - a.rw_ms)
    }

    /// Real-world time at in-game time `ig_ms`. A moment at which the game was paused is
    /// placed at the start of the pause.
    pub fn rw_at(&self, ig_ms: f64) -> f64 {
        let (first, last) = (self.knots[0], self.knots[self.knots.len() - 1]);
        if ig_ms <= first.ig_ms {
            return first.rw_ms - (first.ig_ms - ig_ms) / self.speed_before;
        }
        if ig_ms > last.ig_ms {
            return last.rw_ms + (ig_ms - last.ig_ms) / self.speed_after;
        }
        let i = self.knots.partition_point(|k| k.ig_ms < ig_ms);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        a.rw_ms + (b.rw_ms - a.rw_ms) * (ig_ms - a.ig_ms) / (b.ig_ms - a.ig_ms)
    }

    /// Real-world time at which the game started (in-game time 0).
    pub fn game_start_rw_ms(&self) -> f64 {
        self.rw_at(0.0)
    }

    /// Game speed (in-game ms per real-world ms) while in-game time `ig_ms` ran.
    pub fn speed_at(&self, ig_ms: f64) -> f64 {
        let (first, last) = (self.knots[0], self.knots[self.knots.len() - 1]);
        if ig_ms < first.ig_ms {
            return self.speed_before;
        }
        if ig_ms >= last.ig_ms {
            return self.speed_after;
        }
        let i = self.knots.partition_point(|k| k.ig_ms <= ig_ms);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        (b.ig_ms - a.ig_ms) / (b.rw_ms - a.rw_ms)
    }

    pub fn pauses(&self) -> Vec<Pause> {
        self.knots
            .windows(2)
            .filter(|w| w[1].ig_ms == w[0].ig_ms && w[1].rw_ms > w[0].rw_ms)
            .map(|w| Pause {
                ig_ms: w[0].ig_ms,
                rw_start_ms: w[0].rw_ms,
                duration_ms: w[1].rw_ms - w[0].rw_ms,
            })
            .collect()
    }

    /// Number of stretches with their own speed (pauses not counted).
    pub fn segments(&self) -> usize {
        self.knots
            .windows(2)
            .filter(|w| w[1].ig_ms > w[0].ig_ms)
            .count()
            .max(1)
    }
}

/// An in-game moment and the real-world time it happened at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimePair {
    pub ig_ms: f64,
    pub rw_ms: f64,
    /// How far off `rw_ms` may be by how it was measured: half the time between the two
    /// screen readings a drop fell between (125 ms at 4 frames per second).
    pub uncertainty_ms: f64,
}

/// A pair belongs to a straight segment if it is within its own uncertainty plus this
/// (real-world ms): capture timing jitter and rendering delay. On the sample game 90% of
/// matches are within 130 ms of the fitted line.
pub const SEGMENT_SLACK_MS: f64 = 125.0;

fn pair_tol(p: &TimePair) -> f64 {
    SEGMENT_SLACK_MS + p.uncertainty_ms
}
/// Consecutive pairs needed to start a segment.
const SEED_PAIRS: usize = 4;
/// Shorter segments are treated as wrong matches.
const MIN_SEGMENT_PAIRS: usize = 5;
/// A frozen screen must account for at least this share of a jump to be taken as the pause.
const MIN_FROZEN_SHARE: f64 = 0.5;
/// Shorter jumps between segments are a changing speed, not a pause (real-world ms).
const MIN_PAUSE_MS: f64 = 2_000.0;
/// Segment slopes may be this far outside the speeds searched (rounding, drift).
const SPEED_MARGIN: f64 = 1.1;
/// The game speed before the first and after the last pair is taken from a segment
/// covering at least this much in-game time, if there is one.
const MIN_EXTRAPOLATION_SPAN_MS: f64 = 60_000.0;
/// Pairs beyond the first or last segment are looked for this many tolerances from it.
const EXTENSION_SEARCH: f64 = 4.0;

/// A straight run of pairs: rw = a + b * ig (b = 1 / speed).
#[derive(Debug, Clone)]
struct Segment {
    pairs: Vec<TimePair>,
    a: f64,
    b: f64,
}

impl Segment {
    fn fit(pairs: Vec<TimePair>) -> Segment {
        let (a, b) = least_squares(&pairs);
        Segment { pairs, a, b }
    }

    fn rw(&self, ig: f64) -> f64 {
        self.a + self.b * ig
    }

    fn ig(&self, rw: f64) -> f64 {
        (rw - self.a) / self.b
    }

    fn resid(&self, p: &TimePair) -> f64 {
        p.rw_ms - self.rw(p.ig_ms)
    }

    fn first_ig(&self) -> f64 {
        self.pairs[0].ig_ms
    }

    fn last_ig(&self) -> f64 {
        self.pairs[self.pairs.len() - 1].ig_ms
    }
}

fn least_squares(pairs: &[TimePair]) -> (f64, f64) {
    let n = pairs.len() as f64;
    let mx = pairs.iter().map(|p| p.ig_ms).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.rw_ms).sum::<f64>() / n;
    let sxx: f64 = pairs.iter().map(|p| (p.ig_ms - mx).powi(2)).sum();
    let sxy: f64 = pairs.iter().map(|p| (p.ig_ms - mx) * (p.rw_ms - my)).sum();
    let b = if sxx > 0.0 { sxy / sxx } else { 1.0 };
    (my - b * mx, b)
}

fn consistent(pairs: &[TimePair]) -> Option<Segment> {
    let seg = Segment::fit(pairs.to_vec());
    (seg.b > 0.0 && pairs.iter().all(|p| seg.resid(p).abs() <= pair_tol(p))).then_some(seg)
}

/// Splits pairs (sorted by in-game time) into straight segments, dropping pairs that fit
/// neither the current segment nor start a new one.
fn segment(pairs: &[TimePair]) -> Vec<Segment> {
    let mut segments = vec![];
    let mut i = 0;
    while i + SEED_PAIRS <= pairs.len() {
        let Some(mut seg) = consistent(&pairs[i..i + SEED_PAIRS]) else {
            i += 1;
            continue;
        };
        let mut misses: Vec<TimePair> = vec![];
        let mut j = i + SEED_PAIRS;
        let mut next = pairs.len();
        while j < pairs.len() {
            let p = pairs[j];
            // A young segment's slope is not known well yet, so a pair belongs to it if
            // one line still fits all of them, not only if the current line fits it.
            let mut grown = seg.pairs.clone();
            grown.push(p);
            if let Some(longer) = consistent(&grown) {
                seg = longer;
                misses.clear();
            } else {
                misses.push(p);
                if misses.len() >= SEED_PAIRS
                    && consistent(&misses[misses.len() - SEED_PAIRS..]).is_some()
                {
                    next = j + 1 - SEED_PAIRS;
                    break;
                }
            }
            j += 1;
        }
        segments.push(seg);
        i = next;
    }
    segments
}

/// Fits a monotonic piecewise-linear mapping to time pairs. Pairs off every straight run
/// (wrong matches) are ignored, and so are runs that contradict the others (time running
/// backwards or faster than any game speed). A jump between two runs is placed where the
/// screen stood still (`rows`), as that is what a pause looks like; otherwise the runs are
/// joined where they cross, or by a straight line. None if no segment could be fitted.
pub fn fit_mapping(pairs: &[TimePair], rows: &[CsvRow]) -> Option<TimeMapping> {
    let mut sorted = pairs.to_vec();
    sorted.sort_by(|a, b| a.ig_ms.total_cmp(&b.ig_ms));
    // For comparing segments with each other: the tolerance of a typical pair.
    let mut tols: Vec<f64> = sorted.iter().map(pair_tol).collect();
    tols.sort_by(f64::total_cmp);
    let tol = *tols.get(tols.len() / 2)?;

    // Segment once more without the rejected runs, so the runs on either side of a
    // rejected one can join up again.
    let kept: Vec<TimePair> = chain(segment(&sorted), tol)
        .into_iter()
        .flat_map(|s| s.pairs)
        .collect();
    let segments = chain(segment(&kept), tol);
    let first = segments.first()?;
    let last = segments.last()?;

    let start = Knot {
        rw_ms: first.rw(first.first_ig()),
        ig_ms: first.first_ig(),
    };
    let end = Knot {
        rw_ms: last.rw(last.last_ig()),
        ig_ms: last.last_ig(),
    };
    let head: Vec<TimePair> = sorted
        .iter()
        .rev()
        .filter(|p| p.ig_ms < start.ig_ms)
        .copied()
        .collect();
    let tail: Vec<TimePair> = sorted
        .iter()
        .filter(|p| p.ig_ms > end.ig_ms)
        .copied()
        .collect();

    let mut knots = vec![];
    knots.extend(extend(start, first, &head, tol));
    knots.push(start);
    for w in segments.windows(2) {
        knots.extend(join(&w[0], &w[1], rows, tol));
    }
    knots.push(end);
    knots.extend(extend(end, last, &tail, tol));
    // Rounding can leave a knot a hair behind its predecessor.
    for i in 1..knots.len() {
        knots[i].rw_ms = knots[i].rw_ms.max(knots[i - 1].rw_ms);
        knots[i].ig_ms = knots[i].ig_ms.max(knots[i - 1].ig_ms);
    }
    // A short segment's slope is not reliable enough to extrapolate far.
    let long_enough = |s: &&Segment| s.last_ig() - s.first_ig() >= MIN_EXTRAPOLATION_SPAN_MS;
    let speed_before = 1.0 / segments.iter().find(long_enough).unwrap_or(first).b;
    let speed_after = 1.0 / segments.iter().rev().find(long_enough).unwrap_or(last).b;
    TimeMapping::from_knots(knots, speed_before, speed_after).ok()
}

fn plausible_speed(b: f64) -> bool {
    (MIN_SPEED / SPEED_MARGIN..=MAX_SPEED * SPEED_MARGIN).contains(&(1.0 / b))
}

/// The chain of segments with the most pairs in which time never runs backwards or
/// faster than `MAX_SPEED`, within `tol`.
fn chain(segments: Vec<Segment>, tol: f64) -> Vec<Segment> {
    let segments: Vec<Segment> = segments
        .into_iter()
        .filter(|s| s.pairs.len() >= MIN_SEGMENT_PAIRS && plausible_speed(s.b))
        .collect();
    let compatible = |a: &Segment, b: &Segment| {
        let (ta, tb) = (a.last_ig(), b.first_ig());
        b.rw(tb) - a.rw(ta) >= (tb - ta) / (MAX_SPEED * SPEED_MARGIN) - tol
    };
    // best[j]: (pairs in the best chain ending with segment j, previous segment).
    let mut best: Vec<(usize, Option<usize>)> = vec![];
    for j in 0..segments.len() {
        let prev = (0..j)
            .filter(|&i| compatible(&segments[i], &segments[j]))
            .max_by_key(|&i| best[i].0);
        best.push((
            segments[j].pairs.len() + prev.map_or(0, |i| best[i].0),
            prev,
        ));
    }
    let mut at = (0..segments.len()).max_by_key(|&j| best[j].0);
    let mut keep = vec![false; segments.len()];
    while let Some(j) = at {
        keep[j] = true;
        at = best[j].1;
    }
    segments
        .into_iter()
        .zip(keep)
        .filter_map(|(s, k)| k.then_some(s))
        .collect()
}

/// A knot beyond the first or last segment, through the pairs there (nearest first) that
/// continue it in a straight line from `anchor`. None for fewer than two such pairs or
/// pairs that do not line up.
fn extend(anchor: Knot, seg: &Segment, beyond: &[TimePair], tol: f64) -> Option<Knot> {
    // Wrong matches are far off the segment's own extension.
    let near: Vec<(f64, f64, f64)> = beyond
        .iter()
        .filter(|p| seg.resid(p).abs() <= EXTENSION_SEARCH * tol)
        .map(|p| (p.ig_ms - anchor.ig_ms, p.rw_ms - anchor.rw_ms, pair_tol(p)))
        .collect();
    if near.len() < 2 {
        return None;
    }
    let sxx: f64 = near.iter().map(|d| d.0 * d.0).sum();
    let sxy: f64 = near.iter().map(|d| d.0 * d.1).sum();
    let b = sxy / sxx;
    let fits = near.iter().all(|d| (d.1 - b * d.0).abs() <= d.2);
    let far = near[near.len() - 1].0;
    (fits && plausible_speed(b)).then_some(Knot {
        rw_ms: anchor.rw_ms + b * far,
        ig_ms: anchor.ig_ms + far,
    })
}

/// Knots between the end of segment `a` and the start of segment `b`.
fn join(a: &Segment, b: &Segment, rows: &[CsvRow], tol: f64) -> Vec<Knot> {
    let (ta, tb) = (a.last_ig(), b.first_ig());
    let gap_start = Knot {
        rw_ms: a.rw(ta),
        ig_ms: ta,
    };
    let gap_end = Knot {
        rw_ms: b.rw(tb),
        ig_ms: tb,
    };
    let jump = |t: f64| b.rw(t) - a.rw(t);

    // Speed change: the lines cross inside the gap.
    if (jump(ta) <= 0.0) != (jump(tb) <= 0.0) || (jump(ta).abs() <= tol && jump(tb).abs() <= tol) {
        let t = if a.b != b.b {
            ((b.a - a.a) / (a.b - b.b)).clamp(ta, tb)
        } else {
            (ta + tb) / 2.0
        };
        return vec![Knot {
            rw_ms: a.rw(t),
            ig_ms: t,
        }];
    }

    // Pause: b runs later than a; look for the screen standing still in the gap.
    if jump(ta) > 0.0 && jump(tb) >= MIN_PAUSE_MS {
        if let Some((start, end)) = longest_frozen_run(rows, gap_start.rw_ms, gap_end.rw_ms) {
            let t = a.ig(start).clamp(ta, tb);
            let length = jump(t);
            if end - start >= MIN_FROZEN_SHARE * length {
                return vec![
                    Knot {
                        rw_ms: a.rw(t),
                        ig_ms: t,
                    },
                    Knot {
                        rw_ms: a.rw(t) + length,
                        ig_ms: t,
                    },
                ];
            }
        }
    }

    // Otherwise time ran slower (or faster) somewhere in the gap; join the ends directly.
    vec![
        gap_start,
        Knot {
            rw_ms: gap_end.rw_ms.max(gap_start.rw_ms),
            ig_ms: tb,
        },
    ]
}

/// The longest run of identical consecutive readings within [from, to] (real-world ms), as
/// the timestamps of its first and last frame.
fn longest_frozen_run(rows: &[CsvRow], from: f64, to: f64) -> Option<(f64, f64)> {
    let lo = rows.partition_point(|r| (r.timestamp_ms as f64) < from);
    let hi = rows.partition_point(|r| (r.timestamp_ms as f64) <= to);
    if hi <= lo + 1 {
        return None;
    }
    let same = |x: &CsvRow, y: &CsvRow| {
        CsvRow {
            timestamp_ms: 0,
            ..x.clone()
        } == CsvRow {
            timestamp_ms: 0,
            ..y.clone()
        }
    };
    let mut best: Option<(f64, f64)> = None;
    let mut run_start = lo;
    for i in lo + 1..=hi {
        if i == hi || !same(&rows[i], &rows[run_start]) {
            let start = rows[run_start].timestamp_ms as f64;
            let end = rows[i - 1].timestamp_ms as f64;
            if best.is_none_or(|(s, e)| end - start > e - s) {
                best = Some((start, end));
            }
            run_start = i;
        }
    }
    best.filter(|(s, e)| e > s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(f: impl Fn(f64) -> f64, igs: impl Iterator<Item = u32>) -> Vec<TimePair> {
        igs.map(|t| {
            let ig = t as f64 * 1000.0;
            TimePair {
                ig_ms: ig,
                rw_ms: f(ig),
                uncertainty_ms: 125.0,
            }
        })
        .collect()
    }

    #[test]
    fn linear_mapping_round_trips() {
        let m = TimeMapping::linear(10_000.0, 1.7);
        assert_eq!(m.ig_at(10_000.0), 0.0);
        assert!((m.ig_at(11_000.0) - 1_700.0).abs() < 1e-9);
        assert!((m.rw_at(1_700.0) - 11_000.0).abs() < 1e-9);
        assert!(m.ig_at(9_000.0) < 0.0);
        assert_eq!(m.segments(), 1);
        assert!(m.pauses().is_empty());
    }

    #[test]
    fn rejects_knots_running_backwards() {
        let k = |rw, ig| Knot {
            rw_ms: rw,
            ig_ms: ig,
        };
        assert!(TimeMapping::from_knots(vec![k(0.0, 0.0), k(10.0, 5.0)], 1.0, 1.0).is_ok());
        assert!(TimeMapping::from_knots(vec![k(0.0, 5.0), k(10.0, 0.0)], 1.0, 1.0).is_err());
        assert!(TimeMapping::from_knots(vec![], 1.0, 1.0).is_err());
    }

    #[test]
    fn fits_speed_change_and_drops_outliers() {
        // 1.5 until ig 100 s, then 2.0; one wrong pair.
        let f = |ig: f64| {
            if ig < 100_000.0 {
                ig / 1.5
            } else {
                100_000.0 / 1.5 + (ig - 100_000.0) / 2.0
            }
        };
        let mut p = pairs(f, (2..200).step_by(7));
        p[10].rw_ms += 9_000.0;
        let m = fit_mapping(&p, &[]).unwrap();
        assert!((m.speed_at(50_000.0) - 1.5).abs() < 1e-6);
        assert!((m.speed_at(150_000.0) - 2.0).abs() < 1e-6);
        for ig in [0.0, 60_000.0, 100_000.0, 180_000.0, 250_000.0] {
            assert!((m.rw_at(ig) - f(ig)).abs() < 1.0, "ig {}", ig);
        }
        assert!(m.pauses().is_empty());
    }

    #[test]
    fn places_pause_where_screen_stands_still() {
        // 10 s pause at real-world 60 s; the screen is frozen from 60 s to 70 s.
        let f = |ig: f64| {
            let rw = ig / 1.7;
            if rw < 60_000.0 {
                rw
            } else {
                rw + 10_000.0
            }
        };
        let p = pairs(f, (1..200).step_by(5).filter(|t| !(95..110).contains(t)));
        let rows: Vec<CsvRow> = (0..400)
            .map(|i| {
                let ts = i * 250;
                let frozen = (60_000..=70_000).contains(&ts);
                let food = if frozen { 1 } else { ts as u32 };
                CsvRow {
                    timestamp_ms: ts,
                    food: Some(food),
                    wood: None,
                    gold: None,
                    stone: None,
                    pop_curr: None,
                    pop_max: None,
                    pop_vils: None,
                    idle_vils: None,
                    housing: None,
                }
            })
            .collect();
        let m = fit_mapping(&p, &rows).unwrap();
        let pauses = m.pauses();
        assert_eq!(pauses.len(), 1, "{:?}", m);
        assert!((pauses[0].duration_ms - 10_000.0).abs() < 1.0);
        assert!((pauses[0].rw_start_ms - 60_000.0).abs() < 300.0);
        assert!((m.ig_at(65_000.0) - 60_000.0 * 1.7).abs() < 500.0);
        assert!((m.rw_at(150_000.0) - f(150_000.0)).abs() < 1.0);
    }
}
