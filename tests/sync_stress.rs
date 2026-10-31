//! Sync stress tests: the real sample CSV is distorted in known ways (game speed, pauses,
//! a speed that changes or drifts, unread/garbled values, late capture start) and the time
//! mapping must either recover the real-world time of every replay spend, or report failure.
//! Ignored tests are known bugs; run them with `cargo test --test sync_stress -- --ignored`.
mod common;

use aoe2_squire::sync::TimeMapping;
use aoe2_squire::types::Age;
use aoe2_squire::validation;
use common::Csv;
use std::path::PathBuf;
use std::sync::OnceLock;

/// DE "Normal" game speed; the sample game was played at it.
const NORMAL_SPEED: f64 = 1.7;
const SPEED_REL_TOL: f64 = 0.01;
const GAME_START_TOL_MS: f64 = 500.0;
/// How far a replay spend may be placed from where it really shows on screen.
const PLACEMENT_TOL_MS: f64 = 1000.0;
/// The sanity report's spend alignment on a recovered scenario (99.5% on the sample game;
/// spends from before a late capture start are left out).
const MIN_SPEND_ALIGNMENT: f64 = 0.97;
/// Capture rate, for scenarios played back faster than the sample game.
const CAPTURE_INTERVAL_MS: u64 = 240;

const FOOD: usize = 1;
const WOOD: usize = 3;

struct Baseline {
    csv: Csv,
    mapping: TimeMapping,
    /// Real-world time of the first spend and the Feudal click.
    first_spend_rw: u64,
    feudal_rw: u64,
    /// Known replay events (in-game ms) reported by name when misplaced.
    checkpoints: Vec<(&'static str, f64)>,
    /// Every replay spend (in-game ms); all of them are checked in every scenario.
    spends: Vec<f64>,
}

impl Baseline {
    /// Real-world time of an in-game timestamp according to the unaltered calibration.
    fn rw_of(&self, ig_ms: f64) -> f64 {
        self.mapping.rw_at(ig_ms)
    }
}

fn replay() -> PathBuf {
    common::test_bench("1v1-RM-sample.aoe2record")
}

fn baseline() -> &'static Baseline {
    static BASELINE: OnceLock<Baseline> = OnceLock::new();
    BASELINE.get_or_init(|| {
        let csv = Csv::load();
        let analysis =
            common::analyze(&csv.write("baseline"), Some(&replay())).expect("baseline failed");
        let cal = analysis.calibration.expect("baseline calibration missing");
        let events = || analysis.merged.iter().filter_map(|r| r.event());
        let spends: Vec<f64> = events()
            .filter(|e| {
                e.cost()
                    .is_some_and(|c| c.food + c.wood + c.gold + c.stone > 0)
            })
            .map(|e| e.timestamp_ms() as f64)
            .collect();
        let age_click = |age| {
            events()
                .find(|e| e.age_researched() == Some(age))
                .unwrap_or_else(|| panic!("no {:?} Age click in the sample replay", age))
                .timestamp_ms() as f64
        };
        let (feudal, castle) = (age_click(Age::Feudal), age_click(Age::Castle));
        let last_event = events().next_back().unwrap().timestamp_ms() as f64;
        Baseline {
            first_spend_rw: cal.mapping.rw_at(spends[0]) as u64,
            feudal_rw: cal.mapping.rw_at(feudal) as u64,
            checkpoints: vec![
                ("first spend", spends[0]),
                ("Feudal click", feudal),
                ("Castle click", castle),
                ("last event", last_event),
            ],
            spends,
            mapping: cal.mapping,
            csv,
        }
    })
}

#[derive(PartialEq)]
enum Expect {
    /// Calibration must succeed and be correct.
    Recover,
    /// Recovering is ideal, but returning an error is an acceptable, honest outcome.
    RecoverOrReject,
}

/// `to_scenario_rw` maps an unaltered real-world timestamp onto the scenario's real-world
/// timeline; `speed` is the scenario's true game speed if it is constant.
fn check(
    name: &str,
    csv: &Csv,
    speed: Option<f64>,
    to_scenario_rw: impl Fn(f64) -> f64,
    expect: Expect,
) {
    let b = baseline();
    let analysis = match common::analyze(&csv.write(name), Some(&replay())) {
        Ok(analysis) => analysis,
        Err(e) => {
            assert!(
                expect == Expect::RecoverOrReject,
                "{}: calibration failed: {}",
                name,
                e
            );
            return;
        }
    };
    let cal = analysis.calibration.as_ref().expect("calibration missing");
    let m = &cal.mapping;

    let mut problems = vec![];
    if let Some(speed) = speed {
        for (label, ig) in &b.checkpoints {
            let got = m.speed_at(*ig);
            if (got - speed).abs() > SPEED_REL_TOL * speed {
                problems.push(format!(
                    "speed {:.4} at {}, expected {:.4}",
                    got, label, speed
                ));
            }
        }
    }
    let start_err = m.game_start_rw_ms() - to_scenario_rw(b.mapping.game_start_rw_ms());
    if start_err.abs() > GAME_START_TOL_MS {
        problems.push(format!("game start off by {:+.0} ms", start_err));
    }
    for (label, ig_ms) in &b.checkpoints {
        let err = m.rw_at(*ig_ms) - to_scenario_rw(b.rw_of(*ig_ms));
        if err.abs() > PLACEMENT_TOL_MS {
            problems.push(format!("{} placed {:+.0} ms off", label, err));
        }
    }
    let first_rw = csv.first_ts() as f64;
    let placement_errors: Vec<(f64, f64)> = b
        .spends
        .iter()
        .map(|&ig| (ig, to_scenario_rw(b.rw_of(ig))))
        .filter(|&(_, rw)| rw >= first_rw)
        .map(|(ig, rw)| (ig, m.rw_at(ig) - rw))
        .collect();
    let misplaced: Vec<(f64, f64)> = placement_errors
        .iter()
        .copied()
        .filter(|(_, err)| err.abs() > PLACEMENT_TOL_MS)
        .collect();
    if !misplaced.is_empty() {
        let examples: Vec<String> = misplaced
            .iter()
            .take(5)
            .map(|(ig, err)| format!("ig {:.0} ms: {:+.0} ms", ig, err))
            .collect();
        problems.push(format!(
            "{} of {} spends misplaced, e.g. {}",
            misplaced.len(),
            b.spends.len(),
            examples.join(", ")
        ));
    }
    // Spends from before the capture started cannot be on screen.
    let first_frame_ig = analysis
        .merged
        .iter()
        .find(|o| o.reading().is_some())
        .map_or(0, |o| o.in_game_ms());
    let s = validation::check(&analysis.merged)
        .spends
        .expect("spend alignment");
    let unseen = s
        .unmatched
        .iter()
        .filter(|f| f.in_game_ms < first_frame_ig)
        .count();
    let score = s.matched as f64 / (s.total - unseen) as f64;
    if score < MIN_SPEND_ALIGNMENT {
        problems.push(format!("sanity report spend alignment {:.3}", score));
    }
    let worst = placement_errors
        .iter()
        .map(|(_, e)| *e)
        .max_by(|x, y| x.abs().total_cmp(&y.abs()))
        .unwrap_or(0.0);
    println!(
        "RESULT {}: game start {:+.0} ms, worst spend placement {:+.0} ms, spend alignment \
         {:.1}% ({})",
        name,
        start_err,
        worst,
        score * 100.0,
        cal.summary()
    );
    assert!(
        problems.is_empty(),
        "{}: calibration wrong ({}):\n  {}",
        name,
        cal.summary(),
        problems.join("\n  ")
    );
}

/// For scenarios that leave the real-world timeline unchanged.
fn check_unchanged(name: &str, csv: &Csv, expect: Expect) {
    let b = baseline();
    check(name, csv, Some(b.mapping.speed_at(0.0)), |t| t, expect);
}

#[test]
fn baseline_matches_normal_speed() {
    let b = baseline();
    let speed = b.mapping.speed_at(0.0);
    assert!(
        (speed - NORMAL_SPEED).abs() <= SPEED_REL_TOL * NORMAL_SPEED,
        "baseline speed {:.4} is not DE normal speed {}",
        speed,
        NORMAL_SPEED
    );
    assert_eq!(b.mapping.segments(), 1, "{:?}", b.mapping);
    assert!(b.mapping.pauses().is_empty());
    check_unchanged("baseline", &b.csv, Expect::Recover);
}

fn check_speed(name: &str, target_speed: f64) {
    let b = baseline();
    let k = b.mapping.speed_at(0.0) / target_speed;
    let t0 = b.csv.first_ts() as f64;
    let mut csv = b.csv.clone();
    csv.stretch(k);
    csv.resample(CAPTURE_INTERVAL_MS);
    check(
        name,
        &csv,
        Some(target_speed),
        |t| t0 + (t - t0) * k,
        Expect::Recover,
    );
}

#[test]
fn speed_1_7_rescaled_exactly() {
    check_speed("speed_1_7", NORMAL_SPEED);
}

#[test]
fn speed_1_0() {
    check_speed("speed_1_0", 1.0);
}

#[test]
fn speed_1_5() {
    check_speed("speed_1_5", 1.5);
}

#[test]
fn speed_2_0() {
    check_speed("speed_2_0", 2.0);
}

/// Pauses of (real-world time in the unaltered CSV, duration).
fn check_pauses(name: &str, pauses: &[(u64, u64)]) {
    let b = baseline();
    let mut csv = b.csv.clone();
    let mut at: Vec<(f64, u64)> = pauses
        .iter()
        .map(|&(rw, d)| (b.csv.ts(b.csv.index_at(rw)) as f64, d))
        .collect();
    at.sort_by(|x, y| x.0.total_cmp(&y.0));
    // Latest first, so earlier row indices stay valid.
    for &(rw, d) in at.iter().rev() {
        csv.pause(b.csv.index_at(rw as u64), d);
    }
    let to_scenario_rw = |t: f64| {
        t + at
            .iter()
            .filter(|(rw, _)| t >= *rw)
            .map(|(_, d)| *d as f64)
            .sum::<f64>()
    };
    check(
        name,
        &csv,
        Some(b.mapping.speed_at(0.0)),
        to_scenario_rw,
        Expect::Recover,
    );
}

fn check_pause(name: &str, before_feudal: bool, duration_ms: u64) {
    let b = baseline();
    let at = if before_feudal {
        (b.first_spend_rw + b.feudal_rw) / 2
    } else {
        b.feudal_rw + 60_000
    };
    check_pauses(name, &[(at, duration_ms)]);
}

#[test]
fn pause_8s_before_feudal() {
    check_pause("pause_8s_before_feudal", true, 8_000);
}

#[test]
fn pause_30s_before_feudal() {
    check_pause("pause_30s_before_feudal", true, 30_000);
}

#[test]
fn pause_8s_after_feudal() {
    check_pause("pause_8s_after_feudal", false, 8_000);
}

#[test]
fn pause_30s_after_feudal() {
    check_pause("pause_30s_after_feudal", false, 30_000);
}

#[test]
fn multiple_pauses() {
    // 10 s before Feudal, 40 s in Castle Age, 3 s near the end (as in the sample games to
    // record, test_bench/README.md).
    let b = baseline();
    let t0 = b.csv.first_ts();
    check_pauses(
        "multiple_pauses",
        &[
            ((b.first_spend_rw + b.feudal_rw) / 2, 10_000),
            (t0 + 700_000, 40_000),
            (t0 + 1_100_000, 3_000),
        ],
    );
}

/// Real-world time after the scenario's playback speed `factor(t)` (relative to the
/// sample's own speed) is applied from the first row on, integrated in 1 s steps.
fn warp_by(factor: impl Fn(f64) -> f64, t0: f64) -> impl Fn(f64) -> f64 {
    const STEP: f64 = 1_000.0;
    move |t: f64| {
        let (mut rw, mut s) = (t0, t0);
        while s + STEP <= t {
            rw += STEP / factor(s - t0);
            s += STEP;
        }
        rw + (t - s) / factor(s - t0)
    }
}

fn check_warped(name: &str, factor: impl Fn(f64) -> f64) {
    let b = baseline();
    let warp = warp_by(factor, b.csv.first_ts() as f64);
    let mut csv = b.csv.clone();
    csv.warp(&warp);
    csv.resample(CAPTURE_INTERVAL_MS);
    check(name, &csv, None, warp, Expect::Recover);
}

#[test]
fn speed_changes_mid_game() {
    // Normal (1.7) until Castle Age, then Fast (2.0).
    let b = baseline();
    let switch = b.rw_of(b.checkpoints[2].1) - b.csv.first_ts() as f64 - 30_000.0;
    let fast = 2.0 / b.mapping.speed_at(0.0);
    check_warped("speed_changes_mid_game", |t| {
        if t < switch {
            1.0
        } else {
            fast
        }
    });
}

#[test]
fn replay_playback_speed_changes() {
    // Replay playback at 1x, then 2x for five minutes, then 1x again.
    check_warped("replay_playback_speed_changes", |t| {
        if (400_000.0..700_000.0).contains(&t) {
            2.0
        } else {
            1.0
        }
    });
}

#[test]
fn replay_playback_drift() {
    // Playback speed wanders by 1.5% either way every five minutes and slowly speeds up
    // by 2%. The sample game's own playback ran at one constant speed (residual median
    // 70 ms), so this is far rougher than anything seen yet.
    check_warped("replay_playback_drift", |t| {
        1.0 + 0.015 * (t / 300_000.0 * std::f64::consts::TAU).sin() + 0.02 * t / 1_200_000.0
    });
}

#[test]
fn unread_food_before_first_spend() {
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.csv.first_ts() + 1_000);
    csv.rows[i][FOOD] = String::new();
    check_unchanged("unread_food_before_first_spend", &csv, Expect::Recover);
}

#[test]
fn unread_wood_on_first_spend_frame() {
    // The first spend (a house, -25 wood) is not readable on its first frame.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.first_spend_rw);
    csv.rows[i][WOOD] = String::new();
    check_unchanged("unread_wood_on_first_spend_frame", &csv, Expect::Recover);
}

#[test]
fn unread_food_before_feudal_click() {
    // The last frame showing the saved-up food before the Feudal click is unreadable.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.feudal_rw);
    csv.rows[i - 1][FOOD] = String::new();
    check_unchanged("unread_food_before_feudal_click", &csv, Expect::Recover);
}

#[test]
fn unread_values_all_game() {
    // One field in 20 is unreadable, spread over every resource.
    let b = baseline();
    let mut csv = b.csv.clone();
    for (i, r) in csv.rows.iter_mut().enumerate() {
        if i % 20 == 7 {
            r[1 + 2 * ((i / 20) % 4)] = String::new();
        }
    }
    check_unchanged("unread_values_all_game", &csv, Expect::Recover);
}

#[test]
fn food_gathered_before_first_spend() {
    // Villagers gather food before the first spend, so food is above its starting value
    // on the frame where wood first drops. Also what a garbled 200 -> 208 reading looks like.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.first_spend_rw);
    for r in &mut csv.rows[i - 2..=i] {
        r[FOOD] = "208".to_string();
    }
    check_unchanged("food_gathered_before_first_spend", &csv, Expect::Recover);
}

#[test]
fn late_capture_start_before_first_spend() {
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.first_spend_rw - 1_000);
    csv.rows.drain(..i);
    check_unchanged(
        "late_capture_start_before_first_spend",
        &csv,
        Expect::Recover,
    );
}

fn check_late_capture(name: &str, after_first_spend_ms: u64) {
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.first_spend_rw + after_first_spend_ms);
    csv.rows.drain(..i);
    check_unchanged(name, &csv, Expect::Recover);
}

#[test]
fn late_capture_start_after_first_spend() {
    check_late_capture("late_capture_start_after_first_spend", 5_000);
}

#[test]
fn late_capture_start_20s_after_first_spend() {
    check_late_capture("late_capture_start_20s_after_first_spend", 20_000);
}

#[test]
fn unrelated_telemetry_is_rejected() {
    // Telemetry of another game: the sample read backwards (resources rise where the replay
    // spends). Sync must fail rather than invent a mapping.
    let b = baseline();
    let mut csv = b.csv.clone();
    let ts: Vec<String> = csv.rows.iter().map(|r| r[0].clone()).collect();
    csv.rows.reverse();
    for (r, t) in csv.rows.iter_mut().zip(ts) {
        r[0] = t;
    }
    let result = common::analyze(&csv.write("unrelated_telemetry"), Some(&replay()));
    assert!(
        result.is_err(),
        "sync accepted unrelated telemetry: {}",
        result.unwrap().calibration.unwrap().summary()
    );
}
