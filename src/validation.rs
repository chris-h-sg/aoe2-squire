//! Plausibility checks for a captured game that need no ground truth: screen readings that
//! contradict themselves, and replay spends cross-checked against resource drops on screen.
//! Run on a real game with `cargo run --bin sanity_report -- <csv> [replay] [--json]`.

use crate::analysis::format_time;
use crate::types::MergedRow;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const RESOURCES: [&str; 4] = ["food", "wood", "gold", "stone"];
const POP_FIELDS: [&str; 3] = ["pop_curr", "pop_max", "pop_vils"];

/// A spend shows on screen in the frame after it happens; frames are ~425 in-game ms apart
/// at normal speed, and sync is not exact.
const MATCH_BEFORE_MS: u64 = 1_500;
const MATCH_AFTER_MS: u64 = 2_500;
/// Typical delay between a replay event and its drop on screen (about half a frame).
const EXPECTED_DELAY_MS: u64 = 250;
/// Civ discounts and gathering within the same frame make the drop smaller than the cost.
const MIN_MATCHED_FRACTION: f64 = 0.4;
/// A multi-building queue whose share of the drops is below this fraction of its replay cost.
const OVERCLAIM_FRACTION: f64 = 0.7;
/// One-frame increases above this are more than a burst of villager drop-offs.
const MAX_PLAUSIBLE_INCREASE: u32 = 80;
/// Automatic farm reseeding costs 60 wood (less with discounts) without a replay command.
const FARM_RESEED_WOOD: std::ops::RangeInclusive<u32> = 40..=60;
/// Smaller leftover drops are not reported (gathering noise, repairs).
const MIN_REPORTED_DROP: u32 = 20;
/// Population values move in small steps; a one-frame excursion further than this is a misread.
const POP_SPIKE: u32 = 2;
/// Examples listed per section in the text report.
const MAX_EXAMPLES: usize = 10;

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub in_game_ms: u64,
    pub rw_timestamp_ms: u64,
    pub field: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OffsetStats {
    pub median_ms: i64,
    pub p10_ms: i64,
    pub p90_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpendAlignment {
    /// Replay events with a resource cost.
    pub total: usize,
    /// Of those, events with a matching resource drop on screen.
    pub matched: usize,
    pub score: f64,
    /// Time from the replay event to the first matching drop (in-game ms).
    pub offset: Option<OffsetStats>,
    /// Median offset per quarter of the game; a trend means the speed factor is off, a step
    /// means a pause or a sync jump.
    pub offset_by_quarter_ms: Vec<Option<i64>>,
    pub unmatched: Vec<Finding>,
    /// Multi-building queues charged more by the replay parser than the screen shows.
    pub overclaimed: Vec<Finding>,
    pub farm_reseeds: usize,
    /// Drops no replay event explains (walls, market and tributes are not parsed either).
    pub unexplained_drops: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SanityReport {
    pub frames: usize,
    /// Field -> frames without a reading.
    pub missing: BTreeMap<String, usize>,
    /// A value that is off for exactly one frame: A-B-A for resources (which can drop and
    /// recover legitimately), a spike away from both neighbours for population fields.
    pub glitches: Vec<Finding>,
    pub invariant_violations: Vec<Finding>,
    /// Large one-frame increases without a refund (unqueue/delete) nearby.
    pub large_increases: Vec<Finding>,
    /// None without replay events.
    pub spends: Option<SpendAlignment>,
}

struct Frame {
    ig_ms: u64,
    rw_ms: u64,
    values: BTreeMap<&'static str, u32>,
}

struct Spend {
    ig_ms: u64,
    desc: String,
    cost: [u32; 4],
    buildings: u32,
}

/// One field's readings: (frame index, value), skipping frames without a reading.
type Series = Vec<(usize, u32)>;

pub fn check(merged: &[MergedRow]) -> SanityReport {
    let all_fields: Vec<&'static str> = RESOURCES
        .iter()
        .chain(POP_FIELDS.iter())
        .chain(["idle_vils"].iter())
        .copied()
        .collect();

    let mut frames = vec![];
    let mut spends = vec![];
    let mut refunds = vec![];
    for row in merged {
        match row.observation_type.as_str() {
            "ScreenGrab" => frames.push(Frame {
                ig_ms: row.in_game_ms,
                rw_ms: row.rw_timestamp_ms.parse().unwrap_or(0),
                values: all_fields
                    .iter()
                    .filter_map(|&f| field_str(row, f).parse().ok().map(|v| (f, v)))
                    .collect(),
            }),
            "RecEvent" => {
                if row.event_desc == "Unqueue" || row.event_desc.starts_with("Delete") {
                    refunds.push(row.in_game_ms);
                }
                let cost = RESOURCES.map(|r| {
                    field_str(row, r)
                        .trim_start_matches('-')
                        .parse()
                        .unwrap_or(0)
                });
                if cost.iter().any(|&c| c > 0) {
                    spends.push(Spend {
                        ig_ms: row.in_game_ms,
                        desc: row.event_desc.clone(),
                        cost,
                        buildings: row.building_ids.split(',').count().max(1) as u32,
                    });
                }
            }
            _ => {}
        }
    }

    let series: BTreeMap<&'static str, Series> = all_fields
        .iter()
        .map(|&f| {
            let s = frames
                .iter()
                .enumerate()
                .filter_map(|(i, fr)| fr.values.get(f).map(|&v| (i, v)))
                .collect();
            (f, s)
        })
        .collect();

    let missing = all_fields
        .iter()
        .map(|&f| (f.to_string(), frames.len() - series[f].len()))
        .collect();

    let finding = |i: usize, field: &str, detail: String| Finding {
        in_game_ms: frames[i].ig_ms,
        rw_timestamp_ms: frames[i].rw_ms,
        field: field.to_string(),
        detail,
    };

    // Single-frame glitches (idle villagers legitimately blink, so they are left out).
    let mut glitches = vec![];
    let mut glitch_frames: BTreeSet<(&str, usize)> = BTreeSet::new();
    for &f in RESOURCES.iter().chain(POP_FIELDS.iter()) {
        let is_pop = POP_FIELDS.contains(&f);
        for w in series[f].windows(3) {
            let [(_, a), (i, b), (_, c)] = [w[0], w[1], w[2]];
            let glitch = if is_pop {
                b + POP_SPIKE < a.min(c) || b > a.max(c) + POP_SPIKE
            } else {
                a == c && b != a
            };
            if glitch {
                glitches.push(finding(i, f, format!("{} -> {} -> {}", a, b, c)));
                glitch_frames.insert((f, i));
            }
        }
    }

    let invariant_violations = check_invariants(&frames, &finding);

    // Large increases: refunds explain them, glitch recoveries are already reported.
    let mut large_increases = vec![];
    for &f in &RESOURCES {
        for w in series[f].windows(2) {
            let [(prev_i, a), (i, b)] = [w[0], w[1]];
            let refund_nearby = refunds.iter().any(|&t| {
                t + MATCH_AFTER_MS >= frames[i].ig_ms && t <= frames[i].ig_ms + MATCH_BEFORE_MS
            });
            if b > a + MAX_PLAUSIBLE_INCREASE
                && !refund_nearby
                && !glitch_frames.contains(&(f, prev_i))
            {
                large_increases.push(finding(i, f, format!("{} -> {} (+{})", a, b, b - a)));
            }
        }
    }

    let spends = if spends.is_empty() {
        None
    } else {
        Some(align_spends(&frames, &series, &glitch_frames, &spends))
    };

    SanityReport {
        frames: frames.len(),
        missing,
        glitches,
        invariant_violations,
        large_increases,
        spends,
    }
}

fn field_str<'a>(row: &'a MergedRow, field: &str) -> &'a str {
    match field {
        "food" => &row.food,
        "wood" => &row.wood,
        "gold" => &row.gold,
        "stone" => &row.stone,
        "pop_curr" => &row.pop_curr,
        "pop_max" => &row.pop_max,
        "pop_vils" => &row.pop_vils,
        "idle_vils" => &row.idle_vils,
        _ => "",
    }
}

/// Reports the first frame of each run of frames that break the same rule.
fn check_invariants(
    frames: &[Frame],
    finding: &dyn Fn(usize, &str, String) -> Finding,
) -> Vec<Finding> {
    let rules: [(&str, &str, &str, &str); 3] = [
        ("pop_vils", "pop_curr", "villagers > population", ""),
        ("idle_vils", "pop_vils", "idle villagers > villagers", ""),
        (
            "pop_curr",
            "pop_max",
            "population > housing",
            " (possible if houses were destroyed)",
        ),
    ];
    let mut out = vec![];
    for (big, small, name, note) in rules {
        let mut run_start: Option<usize> = None;
        let flush = |start: Option<usize>, end: usize, out: &mut Vec<Finding>| {
            if let Some(s) = start {
                let fr = &frames[s];
                out.push(finding(
                    s,
                    big,
                    format!(
                        "{}: {} {} > {} {} for {} frame(s){}",
                        name,
                        big,
                        fr.values[big],
                        small,
                        fr.values[small],
                        end - s,
                        note
                    ),
                ));
            }
        };
        for (i, fr) in frames.iter().enumerate() {
            let broken =
                matches!((fr.values.get(big), fr.values.get(small)), (Some(b), Some(s)) if b > s);
            if broken && run_start.is_none() {
                run_start = Some(i);
            } else if !broken && run_start.is_some() {
                flush(run_start.take(), i, &mut out);
            }
        }
        flush(run_start, frames.len(), &mut out);
    }
    out
}

struct Drop {
    frame: usize,
    amount: u32,
    left: u32,
}

fn align_spends(
    frames: &[Frame],
    series: &BTreeMap<&'static str, Series>,
    glitch_frames: &BTreeSet<(&str, usize)>,
    spends: &[Spend],
) -> SpendAlignment {
    let mut drops: Vec<Vec<Drop>> = RESOURCES
        .iter()
        .map(|&r| {
            series[r]
                .windows(2)
                .filter(|w| {
                    !glitch_frames.contains(&(r, w[0].0)) && !glitch_frames.contains(&(r, w[1].0))
                })
                .filter(|w| w[1].1 < w[0].1)
                .map(|w| Drop {
                    frame: w[1].0,
                    amount: w[0].1 - w[1].1,
                    left: w[0].1 - w[1].1,
                })
                .collect()
        })
        .collect();
    let in_window = |t: u64, d: &Drop| {
        let ig = frames[d.frame].ig_ms;
        ig + MATCH_BEFORE_MS >= t && ig <= t + MATCH_AFTER_MS
    };
    let finding_at = |t: u64, field: &str, detail: String| Finding {
        in_game_ms: t,
        rw_timestamp_ms: frames
            .iter()
            .find(|f| f.ig_ms >= t)
            .or(frames.last())
            .map_or(0, |f| f.rw_ms),
        field: field.to_string(),
        detail,
    };

    let mut matched = 0;
    let mut offsets: Vec<(u64, i64)> = vec![];
    let mut unmatched = vec![];
    let mut overclaimed = vec![];
    for sp in spends {
        // The replay parser charges multi-building queues once per building; match on one
        // unit's cost and report the difference separately.
        let per_unit = sp.cost.map(|c| c.div_ceil(sp.buildings));
        let mut takes: Vec<(usize, usize, u32)> = vec![];
        let mut first_drop: Option<u64> = None;
        let mut short = vec![];
        for (r, &need) in per_unit.iter().enumerate() {
            if need == 0 {
                continue;
            }
            // Closest to when the drop should show first, so an unrelated drop just before
            // the event (e.g. a farm reseed) is not taken instead of the real one.
            let expected = sp.ig_ms + EXPECTED_DELAY_MS;
            let mut candidates: Vec<usize> = (0..drops[r].len())
                .filter(|&k| in_window(sp.ig_ms, &drops[r][k]) && drops[r][k].left > 0)
                .collect();
            candidates.sort_by_key(|&k| frames[drops[r][k].frame].ig_ms.abs_diff(expected));
            let mut got = 0;
            for k in candidates {
                // A spend that is already covered (costs are often discounted) must not eat
                // into the next spend's drop.
                if got >= need || got as f64 >= MIN_MATCHED_FRACTION * need as f64 {
                    break;
                }
                let d = &drops[r][k];
                let take = d.left.min(need - got);
                takes.push((r, k, take));
                got += take;
                let ig = frames[d.frame].ig_ms;
                first_drop = Some(first_drop.map_or(ig, |f: u64| f.min(ig)));
            }
            if (got as f64) < MIN_MATCHED_FRACTION * need as f64 {
                short.push(r);
            }
        }

        if short.is_empty() {
            matched += 1;
            for (r, k, take) in takes {
                drops[r][k].left -= take;
            }
            if let Some(f) = first_drop {
                offsets.push((sp.ig_ms, f as i64 - sp.ig_ms as i64));
            }
        } else {
            let reasons: Vec<String> = short
                .iter()
                .map(|&r| {
                    let before = series[RESOURCES[r]]
                        .iter()
                        .rev()
                        .find(|(i, _)| frames[*i].ig_ms <= sp.ig_ms)
                        .map(|&(_, v)| v);
                    match before {
                        Some(v) if v < per_unit[r] => format!(
                            "{}: only {} available for {} (command probably had no effect)",
                            RESOURCES[r], v, per_unit[r]
                        ),
                        _ => format!("{}: no drop of ~{} on screen", RESOURCES[r], per_unit[r]),
                    }
                })
                .collect();
            unmatched.push(finding_at(
                sp.ig_ms,
                RESOURCES[short[0]],
                format!("{}: {}", sp.desc, reasons.join("; ")),
            ));
        }

        if sp.buildings > 1 {
            for (r, &claimed) in sp.cost.iter().enumerate() {
                if claimed == 0 {
                    continue;
                }
                let window_drops: u32 = drops[r]
                    .iter()
                    .filter(|d| in_window(sp.ig_ms, d))
                    .map(|d| d.amount)
                    .sum();
                let sharing = spends
                    .iter()
                    .filter(|o| {
                        o.cost[r] > 0
                            && o.ig_ms + MATCH_AFTER_MS + MATCH_BEFORE_MS >= sp.ig_ms
                            && o.ig_ms <= sp.ig_ms + MATCH_AFTER_MS + MATCH_BEFORE_MS
                    })
                    .count()
                    .max(1) as u32;
                let share = window_drops / sharing;
                if (share as f64) < OVERCLAIM_FRACTION * claimed as f64 {
                    overclaimed.push(finding_at(
                        sp.ig_ms,
                        RESOURCES[r],
                        format!(
                            "{}: replay charges {} for {} buildings, screen shows ~{} per event",
                            sp.desc, claimed, sp.buildings, share
                        ),
                    ));
                }
            }
        }
    }

    let mut farm_reseeds = 0;
    let mut unexplained_drops = vec![];
    for (r, list) in drops.iter().enumerate() {
        for d in list.iter().filter(|d| d.left >= MIN_REPORTED_DROP) {
            if RESOURCES[r] == "wood" && FARM_RESEED_WOOD.contains(&d.left) {
                farm_reseeds += 1;
            } else {
                let fr = &frames[d.frame];
                unexplained_drops.push(Finding {
                    in_game_ms: fr.ig_ms,
                    rw_timestamp_ms: fr.rw_ms,
                    field: RESOURCES[r].to_string(),
                    detail: format!("-{} not explained by a replay event", d.left),
                });
            }
        }
    }
    unexplained_drops.sort_by_key(|f| f.in_game_ms);

    let total = spends.len();
    let last_ms = frames.last().map_or(1, |f| f.ig_ms.max(1));
    let offset_by_quarter_ms = (0..4)
        .map(|q| {
            let mut part: Vec<i64> = offsets
                .iter()
                .filter(|(t, _)| (*t * 4 / last_ms).min(3) == q)
                .map(|(_, o)| *o)
                .collect();
            percentile(&mut part, 50)
        })
        .collect();
    let mut all: Vec<i64> = offsets.iter().map(|(_, o)| *o).collect();
    let offset = percentile(&mut all, 50).map(|median_ms| OffsetStats {
        median_ms,
        p10_ms: percentile(&mut all, 10).unwrap(),
        p90_ms: percentile(&mut all, 90).unwrap(),
    });

    SpendAlignment {
        total,
        matched,
        score: matched as f64 / total as f64,
        offset,
        offset_by_quarter_ms,
        unmatched,
        overclaimed,
        farm_reseeds,
        unexplained_drops,
    }
}

fn percentile(values: &mut [i64], p: usize) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[(values.len() - 1) * p / 100])
}

impl SanityReport {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let mut line = |s: String| {
            out.push_str(&s);
            out.push('\n');
        };
        line(format!("=== SANITY REPORT ({} frames) ===", self.frames));
        let missing: Vec<String> = self
            .missing
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(f, n)| format!("{} {}", f, n))
            .collect();
        line(format!(
            "Missing readings: {}",
            if missing.is_empty() {
                "none".to_string()
            } else {
                missing.join(", ")
            }
        ));

        if let Some(s) = &self.spends {
            line(String::new());
            line(format!(
                "Spend alignment: {}/{} replay spends matched a drop on screen ({:.1}%)",
                s.matched,
                s.total,
                s.score * 100.0
            ));
            if let Some(o) = &s.offset {
                line(format!(
                    "  Drop after replay event: median {} ms (p10 {}, p90 {})",
                    o.median_ms, o.p10_ms, o.p90_ms
                ));
            }
            let quarters: Vec<String> = s
                .offset_by_quarter_ms
                .iter()
                .map(|q| q.map_or("-".to_string(), |v| format!("{} ms", v)))
                .collect();
            line(format!(
                "  Median offset per game quarter: {}",
                quarters.join(", ")
            ));
            line(format!(
                "  Farm reseeds (wood drops without a command): {}",
                s.farm_reseeds
            ));
            section(&mut line, "Unmatched spends", &s.unmatched);
            section(
                &mut line,
                "Over-charged multi-building queues",
                &s.overclaimed,
            );
            section(&mut line, "Unexplained drops", &s.unexplained_drops);
        } else {
            line(String::new());
            line("Spend alignment: skipped (no replay)".to_string());
        }

        line(String::new());
        section(&mut line, "Single-frame glitches", &self.glitches);
        section(
            &mut line,
            "Invariant violations",
            &self.invariant_violations,
        );
        section(
            &mut line,
            "Large increases without a refund",
            &self.large_increases,
        );
        out
    }
}

fn section(line: &mut dyn FnMut(String), title: &str, findings: &[Finding]) {
    line(format!("{}: {}", title, findings.len()));
    for f in findings.iter().take(MAX_EXAMPLES) {
        line(format!(
            "  {} (rw {}) {}: {}",
            format_time(f.in_game_ms),
            f.rw_timestamp_ms,
            f.field,
            f.detail
        ));
    }
    if findings.len() > MAX_EXAMPLES {
        line(format!("  ... {} more", findings.len() - MAX_EXAMPLES));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grab(ms: u64, food: &str, wood: &str, pop: (&str, &str, &str, &str)) -> MergedRow {
        MergedRow {
            in_game_ms: ms,
            observation_type: "ScreenGrab".to_string(),
            food: food.to_string(),
            wood: wood.to_string(),
            gold: "100".to_string(),
            stone: "200".to_string(),
            rw_timestamp_ms: (1000 + ms).to_string(),
            pop_curr: pop.0.to_string(),
            pop_max: pop.1.to_string(),
            pop_vils: pop.2.to_string(),
            idle_vils: pop.3.to_string(),
            ..Default::default()
        }
    }

    fn event(ms: u64, desc: &str, food: &str, wood: &str, buildings: &str) -> MergedRow {
        MergedRow {
            in_game_ms: ms,
            observation_type: "RecEvent".to_string(),
            event_desc: desc.to_string(),
            food: food.to_string(),
            wood: wood.to_string(),
            building_ids: buildings.to_string(),
            ..Default::default()
        }
    }

    const POP: (&str, &str, &str, &str) = ("4", "5", "3", "0");

    #[test]
    fn matches_spends_and_reports_offsets() {
        let rows = vec![
            event(0, "Queue Villager", "-50", "", "1"),
            grab(0, "200", "200", POP),
            grab(425, "150", "200", POP),
            event(1000, "Build House", "", "-25", ""),
            grab(850, "150", "200", POP),
            grab(1275, "155", "180", POP),
            event(5000, "Queue Villager", "-50", "", "1"),
            grab(5100, "160", "180", POP),
        ];
        let r = check(&rows);
        let s = r.spends.unwrap();
        assert_eq!((s.matched, s.total), (2, 3));
        assert_eq!(s.offset.unwrap().median_ms, 275);
        assert!(s.unmatched[0].detail.contains("no drop of ~50"));
        assert!(s.unexplained_drops.is_empty());
    }

    #[test]
    fn flags_unaffordable_spends_farm_reseeds_and_unexplained_drops() {
        let rows = vec![
            grab(0, "30", "300", POP),
            event(100, "Queue Villager", "-50", "", "1"),
            grab(425, "30", "240", POP),
            grab(5000, "30", "140", POP),
        ];
        let s = check(&rows).spends.unwrap();
        assert!(s.unmatched[0].detail.contains("only 30 available"));
        assert_eq!(s.farm_reseeds, 1);
        assert_eq!(s.unexplained_drops.len(), 1);
        assert_eq!(
            s.unexplained_drops[0].detail,
            "-100 not explained by a replay event"
        );
    }

    #[test]
    fn flags_overcharged_multi_building_queues() {
        // Two batches of one unit each from two selected stables: the replay charges 160.
        let rows = vec![
            grab(0, "500", "200", POP),
            event(100, "Queue Scout", "-160", "", "7,8"),
            event(200, "Queue Scout", "-160", "", "7,8"),
            grab(425, "340", "200", POP),
        ];
        let s = check(&rows).spends.unwrap();
        assert_eq!(s.matched, 2);
        assert_eq!(s.overclaimed.len(), 2);
        assert!(s.overclaimed[0].detail.contains("screen shows ~80"));
    }

    #[test]
    fn flags_glitches_invariants_and_large_increases() {
        let rows = vec![
            grab(0, "100", "200", ("30", "30", "28", "1")),
            grab(425, "100", "200", ("30", "3", "28", "1")),
            grab(850, "100", "200", ("30", "35", "28", "29")),
            grab(1275, "250", "200", ("30", "35", "28", "0")),
            event(3000, "Unqueue", "", "", "1"),
            grab(3400, "350", "200", ("30", "35", "28", "0")),
        ];
        let r = check(&rows);
        assert_eq!(r.glitches.len(), 1, "{:?}", r.glitches);
        assert_eq!(r.glitches[0].field, "pop_max");
        assert_eq!(r.glitches[0].detail, "30 -> 3 -> 35");
        let rules: Vec<&str> = r
            .invariant_violations
            .iter()
            .map(|f| f.detail.split(':').next().unwrap())
            .collect();
        assert_eq!(
            rules,
            ["idle villagers > villagers", "population > housing"]
        );
        // +150 at 1275 has no refund nearby; +100 at 3400 follows an unqueue.
        assert_eq!(r.large_increases.len(), 1);
        assert_eq!(r.large_increases[0].in_game_ms, 1275);
        assert!(r.spends.is_none());
    }
}
