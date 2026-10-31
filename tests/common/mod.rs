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
