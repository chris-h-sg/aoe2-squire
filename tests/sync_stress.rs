//! Sync calibration stress tests: the real sample CSV is distorted in known ways (game speed,
//! pauses, unread/garbled values, late capture start) and the calibration must either recover
//! the true speed, game start and real-world time of known replay events, or report failure.
//! Ignored tests are known bugs; run them with `cargo test --test sync_stress -- --ignored`.
mod common;

use common::Csv;
use std::path::PathBuf;
use std::sync::OnceLock;

/// DE "Normal" game speed; the sample game was played at it.
const NORMAL_SPEED: f64 = 1.7;
const SPEED_REL_TOL: f64 = 0.01;
const GAME_START_TOL_MS: f64 = 500.0;
/// How far a known replay event may be placed from where it really shows on screen.
const CHECKPOINT_TOL_MS: f64 = 1000.0;

const FOOD: usize = 1;
const WOOD: usize = 3;

struct Baseline {
    csv: Csv,
    speed: f64,
    game_start_rw: f64,
    anchor1_rw: u64,
    anchor2_rw: u64,
    /// Known replay events (in-game ms) whose real-world time is checked in every scenario.
    checkpoints: Vec<(&'static str, u64)>,
}

impl Baseline {
    /// Real-world time of an in-game timestamp according to the unaltered calibration.
    fn rw_of(&self, ig_ms: u64) -> f64 {
        self.game_start_rw + ig_ms as f64 / self.speed
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
        let rec_events = || {
            analysis
                .merged
                .iter()
                .filter(|r| r.observation_type == "RecEvent")
        };
        let castle_ms = rec_events()
            .find(|r| r.event_desc == "Research Castle Age")
            .expect("no Castle Age click in the sample replay")
            .in_game_ms;
        let last_event_ms = rec_events().next_back().unwrap().in_game_ms;
        Baseline {
            speed: cal.speed_factor,
            game_start_rw: cal.game_start_rw_ms,
            anchor1_rw: cal.anchor1_rw,
            anchor2_rw: cal.anchor2_rw,
            checkpoints: vec![
                ("first spend", cal.anchor1_ig),
                ("Feudal click", cal.anchor2_ig),
                ("Castle click", castle_ms),
                ("last event", last_event_ms),
            ],
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
/// timeline; `speed` is the scenario's true game speed.
fn check(name: &str, csv: &Csv, speed: f64, to_scenario_rw: impl Fn(f64) -> f64, expect: Expect) {
    let b = baseline();
    let cal = match common::analyze(&csv.write(name), Some(&replay())) {
        Ok(analysis) => analysis.calibration.expect("calibration missing"),
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

    let mut problems = vec![];
    if (cal.speed_factor - speed).abs() > SPEED_REL_TOL * speed {
        problems.push(format!(
            "speed {:.4}, expected {:.4}",
            cal.speed_factor, speed
        ));
    }
    let start_err = cal.game_start_rw_ms - to_scenario_rw(b.game_start_rw);
    if start_err.abs() > GAME_START_TOL_MS {
        problems.push(format!("game start off by {:+.0} ms", start_err));
    }
    for (label, ig_ms) in &b.checkpoints {
        let got = cal.game_start_rw_ms + *ig_ms as f64 / cal.speed_factor;
        let err = got - to_scenario_rw(b.rw_of(*ig_ms));
        if err.abs() > CHECKPOINT_TOL_MS {
            problems.push(format!("{} placed {:+.0} ms off", label, err));
        }
    }
    assert!(
        problems.is_empty(),
        "{}: calibration wrong (Feudal anchor found: {}):\n  {}",
        name,
        cal.anchor2_rw != 0,
        problems.join("\n  ")
    );
}

/// For scenarios that leave the real-world timeline unchanged.
fn check_unchanged(name: &str, csv: &Csv, expect: Expect) {
    check(name, csv, baseline().speed, |t| t, expect);
}

#[test]
fn baseline_matches_normal_speed() {
    let b = baseline();
    assert!(
        (b.speed - NORMAL_SPEED).abs() <= SPEED_REL_TOL * NORMAL_SPEED,
        "baseline speed {:.4} is not DE normal speed {}",
        b.speed,
        NORMAL_SPEED
    );
    assert!(b.anchor2_rw > 0, "Feudal anchor not found");
    check_unchanged("baseline", &b.csv, Expect::Recover);
}

fn check_speed(name: &str, target_speed: f64) {
    let b = baseline();
    let k = b.speed / target_speed;
    let t0 = b.csv.first_ts() as f64;
    let mut csv = b.csv.clone();
    csv.stretch(k);
    check(
        name,
        &csv,
        target_speed,
        |t| t0 + (t - t0) * k,
        Expect::Recover,
    );
}

#[test]
fn speed_1_7_rescaled_exactly() {
    check_speed("speed_1_7", NORMAL_SPEED);
}

#[test]
#[ignore = "known bug: Feudal anchor search window assumes speed 1.7 (+-10 s), anchor is missed and speed silently falls back to 1.7"]
fn speed_1_5() {
    check_speed("speed_1_5", 1.5);
}

#[test]
#[ignore = "known bug: Feudal anchor search window assumes speed 1.7 (+-10 s), anchor is missed and speed silently falls back to 1.7"]
fn speed_2_0() {
    check_speed("speed_2_0", 2.0);
}

fn check_pause(name: &str, before_feudal: bool, duration_ms: u64) {
    let b = baseline();
    let at = if before_feudal {
        (b.anchor1_rw + b.anchor2_rw) / 2
    } else {
        b.anchor2_rw + 60_000
    };
    let i = b.csv.index_at(at);
    let pause_rw = b.csv.ts(i) as f64;
    let mut csv = b.csv.clone();
    csv.pause(i, duration_ms);
    let to_scenario_rw = |t: f64| {
        if t >= pause_rw {
            t + duration_ms as f64
        } else {
            t
        }
    };
    check(name, &csv, b.speed, to_scenario_rw, Expect::Recover);
}

#[test]
#[ignore = "known bug: a pause between the anchors is folded into the speed factor (1.644), later events drift up to 26 s"]
fn pause_8s_before_feudal() {
    check_pause("pause_8s_before_feudal", true, 8_000);
}

#[test]
#[ignore = "known bug: the pause pushes the Feudal drop out of the search window, silent fallback to 1.7, events after the pause ~30 s off"]
fn pause_30s_before_feudal() {
    check_pause("pause_30s_before_feudal", true, 30_000);
}

#[test]
#[ignore = "known limitation: linear two-point sync has no pause model, events after a pause are off by its length"]
fn pause_8s_after_feudal() {
    check_pause("pause_8s_after_feudal", false, 8_000);
}

#[test]
#[ignore = "known limitation: linear two-point sync has no pause model, events after a pause are off by its length"]
fn pause_30s_after_feudal() {
    check_pause("pause_30s_after_feudal", false, 30_000);
}

#[test]
#[ignore = "known bug: an unread (None) food value is treated as a resource drop (sync.rs:35), game start 1.5 s off"]
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
    // Passes today only because the unread value is itself taken as the drop (see
    // unread_food_before_first_spend), which happens to be the right frame here.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.anchor1_rw);
    csv.rows[i][WOOD] = String::new();
    check_unchanged(
        "unread_wood_on_first_spend_frame",
        &csv,
        Expect::RecoverOrReject,
    );
}

#[test]
#[ignore = "known bug: an unread food value resets prev_food to 0, Feudal drop is missed and speed silently falls back to 1.7"]
fn unread_food_before_feudal_click() {
    // The last frame showing the saved-up food before the Feudal click is unreadable.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.anchor2_rw);
    csv.rows[i - 1][FOOD] = String::new();
    check_unchanged("unread_food_before_feudal_click", &csv, Expect::Recover);
}

#[test]
#[ignore = "known bug: start_food - row.food underflows when food is above its start value (sync.rs:37, panics in debug)"]
fn food_gathered_before_first_spend() {
    // Villagers gather food before the first spend, so food is above its starting value
    // on the frame where wood first drops. Also what a garbled 200 -> 208 reading looks like.
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.anchor1_rw);
    for r in &mut csv.rows[i - 2..=i] {
        r[FOOD] = "208".to_string();
    }
    check_unchanged(
        "food_gathered_before_first_spend",
        &csv,
        Expect::RecoverOrReject,
    );
}

#[test]
fn late_capture_start_before_first_spend() {
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.anchor1_rw - 1_000);
    csv.rows.drain(..i);
    check_unchanged(
        "late_capture_start_before_first_spend",
        &csv,
        Expect::Recover,
    );
}

#[test]
#[ignore = "known bug: the first captured frame is assumed to show starting resources, first spend matched to the wrong event, game start 8 s off"]
fn late_capture_start_after_first_spend() {
    let b = baseline();
    let mut csv = b.csv.clone();
    let i = csv.index_at(b.anchor1_rw + 5_000);
    csv.rows.drain(..i);
    check_unchanged(
        "late_capture_start_after_first_spend",
        &csv,
        Expect::RecoverOrReject,
    );
}
