#![allow(dead_code)]

use aoe2_squire::analysis::run::{analyze_match, MatchAnalysis};
use aoe2_squire::constants::{SMOOTH_VILS_MAX_DURATION_MS, SMOOTH_VILS_MIN_SPIKE};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub fn test_bench(name: &str) -> PathBuf {
    root().join("test_bench").join(name)
}

/// Runs the production analysis path with the production smoothing defaults.
pub fn analyze(csv: &Path, replay: Option<&Path>) -> Result<MatchAnalysis, String> {
    analyze_match(
        csv,
        replay,
        false,
        SMOOTH_VILS_MIN_SPIKE,
        SMOOTH_VILS_MAX_DURATION_MS,
    )
    .map_err(|e| e.to_string())
}

/// Compares two JSON values; numbers are equal within `abs_tol + rel_tol * |expected|`.
/// Returns one line per difference, prefixed with the JSON path.
pub fn json_diff(expected: &Value, actual: &Value, abs_tol: f64, rel_tol: f64) -> Vec<String> {
    let mut diffs = vec![];
    diff_at("$", expected, actual, abs_tol, rel_tol, &mut diffs);
    diffs
}

fn diff_at(path: &str, e: &Value, a: &Value, abs_tol: f64, rel_tol: f64, out: &mut Vec<String>) {
    match (e, a) {
        (Value::Number(en), Value::Number(an)) => {
            let (ef, af) = (en.as_f64().unwrap(), an.as_f64().unwrap());
            if (ef - af).abs() > abs_tol + rel_tol * ef.abs() {
                out.push(format!("{}: expected {}, got {}", path, en, an));
            }
        }
        (Value::Object(eo), Value::Object(ao)) => {
            for (k, ev) in eo {
                match ao.get(k) {
                    Some(av) => diff_at(&format!("{}.{}", path, k), ev, av, abs_tol, rel_tol, out),
                    None => out.push(format!("{}.{}: missing (expected {})", path, k, ev)),
                }
            }
            for k in ao.keys().filter(|k| !eo.contains_key(*k)) {
                out.push(format!("{}.{}: unexpected (got {})", path, k, ao[k]));
            }
        }
        (Value::Array(ea), Value::Array(aa)) => {
            if ea.len() != aa.len() {
                out.push(format!(
                    "{}: expected {} elements, got {}",
                    path,
                    ea.len(),
                    aa.len()
                ));
            }
            for (i, (ev, av)) in ea.iter().zip(aa).enumerate() {
                diff_at(&format!("{}[{}]", path, i), ev, av, abs_tol, rel_tol, out);
            }
        }
        _ if e == a => {}
        _ => out.push(format!("{}: expected {}, got {}", path, e, a)),
    }
}

const CAPTURE_INTERVAL_MS: u64 = 250;

/// The sample telemetry CSV as text, for building distorted variants.
#[derive(Clone)]
pub struct Csv {
    pub header: String,
    pub rows: Vec<Vec<String>>,
}

impl Csv {
    pub fn load() -> Csv {
        let text = std::fs::read_to_string(test_bench("1v1-RM-sample.csv")).unwrap();
        let mut lines = text.lines();
        let header = lines.next().unwrap().to_string();
        let rows = lines
            .filter(|l| !l.is_empty())
            .map(|l| l.split(',').map(str::to_string).collect())
            .collect();
        Csv { header, rows }
    }

    pub fn ts(&self, i: usize) -> u64 {
        self.rows[i][0].parse().unwrap()
    }

    pub fn first_ts(&self) -> u64 {
        self.ts(0)
    }

    /// Index of the first row at or after real-world timestamp `ts`.
    pub fn index_at(&self, ts: u64) -> usize {
        (0..self.rows.len()).find(|&i| self.ts(i) >= ts).unwrap()
    }

    pub fn write(&self, name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{}.csv", name));
        let mut out = self.header.clone() + "\n";
        for r in &self.rows {
            out += &r.join(",");
            out.push('\n');
        }
        std::fs::write(&path, out).unwrap();
        path
    }

    /// Simulates another game speed: real-world time since the first row is scaled by `k`.
    pub fn stretch(&mut self, k: f64) {
        let t0 = self.first_ts() as f64;
        self.warp(|t| t0 + (t - t0) * k);
    }

    /// Moves every row to real-world time `f(timestamp)`; `f` must be increasing.
    pub fn warp(&mut self, f: impl Fn(f64) -> f64) {
        for r in &mut self.rows {
            let ts: f64 = r[0].parse().unwrap();
            r[0] = (f(ts).round() as u64).to_string();
        }
    }

    /// Keeps only rows at least `interval_ms` after the previously kept one, as a capture at
    /// that rate would see a game played back faster than it was recorded.
    pub fn resample(&mut self, interval_ms: u64) {
        let mut last: Option<u64> = None;
        self.rows.retain(|r| {
            let ts: u64 = r[0].parse().unwrap();
            let keep = last.is_none_or(|l| ts >= l + interval_ms);
            if keep {
                last = Some(ts);
            }
            keep
        });
    }

    /// Simulates a pause of `duration_ms` before row `i`: the screen keeps showing the
    /// last values while the capture keeps sampling.
    pub fn pause(&mut self, i: usize, duration_ms: u64) {
        for r in &mut self.rows[i..] {
            let ts: u64 = r[0].parse().unwrap();
            r[0] = (ts + duration_ms).to_string();
        }
        let frozen = self.rows[i - 1].clone();
        let start: u64 = frozen[0].parse().unwrap();
        let fill: Vec<Vec<String>> = (1..=duration_ms / CAPTURE_INTERVAL_MS)
            .map(|n| {
                let mut r = frozen.clone();
                r[0] = (start + n * CAPTURE_INTERVAL_MS).to_string();
                r
            })
            .collect();
        self.rows.splice(i..i, fill);
    }
}
