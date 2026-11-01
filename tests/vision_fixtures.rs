//! Data-driven vision test: runs `pipeline::process_frame` on every image listed in the
//! manifests below, checks each expected field, and prints a per-field accuracy summary.
//!
//! Manifest entries (`images` in each JSON file, paths relative to the manifest):
//! - `use_value_set` (a named set in `value_sets`) or inline `values`: {category: {sub_key: value}}
//! - `expect_no_game: true`: the frame must not be read (process_frame returns None); any
//!   values are then only used by the research extractors (e.g. the enormous-font layout)
//! - `known_failures`: {"category.sub_key" | "game_detected" | "*": "reason"} — fields that are
//!   wrong today. They must still be wrong (strict), so fixing one means removing it here.
//!
//! The total number of correct fields must not drop below `ACCURACY_BASELINE`; regenerate it
//! after an improvement with `UPDATE_SNAPSHOTS=1 cargo test --test vision_fixtures`.
mod common;

use aoe2_squire::{pipeline, types};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const MANIFESTS: [&str; 4] = [
    "test_bench/expected_values.json",
    "test_bench/video_frames/expected_values.json",
    "test_bench/game_frames/game1-castle-vs-ai/expected_values.json",
    "test_bench/game_frames/game2-fast-pauses/expected_values.json",
];
const ACCURACY_BASELINE: &str = "test_bench/snapshots/vision_accuracy.json";
const GAME_DETECTED: &str = "game_detected";

type Values = HashMap<String, HashMap<String, String>>;

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    value_sets: HashMap<String, Values>,
    images: BTreeMap<String, ImageEntry>,
}

#[derive(Deserialize)]
struct ImageEntry {
    use_value_set: Option<String>,
    values: Option<Values>,
    #[serde(default)]
    expect_no_game: bool,
    #[serde(default)]
    known_failures: HashMap<String, String>,
}

struct Fixture {
    path: PathBuf,
    name: String,
    /// "category.sub_key" -> expected value; None for a no-game frame.
    expected: Option<BTreeMap<String, String>>,
    known_failures: HashMap<String, String>,
}

impl Fixture {
    fn known_failure(&self, field: &str) -> Option<&String> {
        self.known_failures
            .get(field)
            .or_else(|| self.known_failures.get("*"))
    }
}

fn load_fixtures() -> Vec<Fixture> {
    let mut fixtures = vec![];
    for manifest_path in MANIFESTS {
        let path = common::root().join(manifest_path);
        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", manifest_path, e)),
        )
        .unwrap_or_else(|e| panic!("{}: {}", manifest_path, e));
        let dir = path.parent().unwrap();

        for (name, entry) in manifest.images {
            let values = match (&entry.use_value_set, entry.values) {
                _ if entry.expect_no_game => None,
                (Some(set), None) => Some(
                    manifest
                        .value_sets
                        .get(set)
                        .unwrap_or_else(|| panic!("{}: unknown value set '{}'", name, set))
                        .clone(),
                ),
                (None, Some(values)) => Some(values),
                _ => panic!(
                    "{}: needs exactly one of use_value_set / values / expect_no_game",
                    name
                ),
            };
            let expected = values.map(|v| {
                v.into_iter()
                    .flat_map(|(cat, sub)| {
                        sub.into_iter()
                            .map(move |(key, val)| (format!("{}.{}", cat, key), val))
                    })
                    .collect()
            });
            fixtures.push(Fixture {
                path: dir.join(&name),
                name: path
                    .parent()
                    .unwrap()
                    .strip_prefix(common::test_bench(""))
                    .unwrap()
                    .join(&name)
                    .to_string_lossy()
                    .replace('\\', "/"),
                expected,
                known_failures: entry.known_failures,
            });
        }
    }
    fixtures
}

struct FieldResult {
    field: String,
    correct: bool,
    detail: String,
}

fn check_fixture(
    f: &Fixture,
    ui_map: &types::UiMap,
    templates: &types::Templates,
) -> Vec<FieldResult> {
    let img = image::open(&f.path).unwrap_or_else(|e| panic!("{}: {}", f.name, e));
    let results = pipeline::process_frame(&img, ui_map, templates);

    let mut out = vec![FieldResult {
        field: GAME_DETECTED.to_string(),
        correct: results.is_some() == f.expected.is_some(),
        detail: format!(
            "got {}, expected {}",
            results.is_some(),
            f.expected.is_some()
        ),
    }];
    if let Some(expected) = &f.expected {
        for (field, want) in expected {
            let (cat, key) = field.split_once('.').unwrap();
            let got = results
                .as_ref()
                .and_then(|r| r.get(cat))
                .and_then(|m| m.get(key));
            out.push(FieldResult {
                field: field.clone(),
                correct: got == Some(want),
                detail: format!("got {:?}, expected {:?}", got, want),
            });
        }
    }
    out
}

#[derive(Default, Clone, Copy, PartialEq)]
struct Tally {
    correct: usize,
    total: usize,
}

#[test]
fn vision_fixtures() {
    let fixtures = load_fixtures();
    let ui_map: types::UiMap =
        serde_json::from_str(&std::fs::read_to_string(common::root().join("ui_map.json")).unwrap())
            .unwrap();
    let templates = pipeline::matcher::load_embedded_templates();

    // Images are processed in parallel; debug builds are slow on the large screenshots.
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Vec<FieldResult>)>> = Mutex::new(vec![]);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(f) = fixtures.get(i) else { break };
                let r = check_fixture(f, &ui_map, &templates);
                results.lock().unwrap().push((i, r));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);

    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut regressions = vec![];
    let mut unexpected_passes = vec![];
    let mut known = vec![];
    for (i, fields) in &results {
        let f = &fixtures[*i];
        for r in fields {
            let t = tally.entry(r.field.clone()).or_default();
            t.total += 1;
            t.correct += r.correct as usize;
            match (r.correct, f.known_failure(&r.field)) {
                (false, None) => {
                    regressions.push(format!("  {} {}: {}", f.name, r.field, r.detail))
                }
                (false, Some(reason)) => known.push(format!(
                    "  {} {}: {} ({})",
                    f.name, r.field, r.detail, reason
                )),
                (true, Some(_)) => unexpected_passes.push(format!("  {} {}", f.name, r.field)),
                (true, None) => {}
            }
        }
    }
    let overall = tally.values().fold(Tally::default(), |a, t| Tally {
        correct: a.correct + t.correct,
        total: a.total + t.total,
    });

    println!("Vision accuracy over {} fixtures:", fixtures.len());
    for (field, t) in &tally {
        println!(
            "  {:<20} {:>4}/{:<4} {:>6.1}%",
            field,
            t.correct,
            t.total,
            100.0 * t.correct as f64 / t.total as f64
        );
    }
    println!(
        "  {:<20} {:>4}/{:<4} {:>6.1}%",
        "TOTAL",
        overall.correct,
        overall.total,
        100.0 * overall.correct as f64 / overall.total as f64
    );
    println!("Known failures ({}):\n{}", known.len(), known.join("\n"));

    let baseline_path = common::root().join(ACCURACY_BASELINE);
    let summary = serde_json::json!({
        "correct": overall.correct,
        "total": overall.total,
        "fields": tally.iter().map(|(k, t)| (k.clone(), serde_json::json!(format!("{}/{}", t.correct, t.total))))
            .collect::<serde_json::Map<_, _>>(),
    });
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&summary).unwrap() + "\n",
        )
        .unwrap();
        eprintln!("Accuracy baseline written to {}", baseline_path.display());
    }

    assert!(
        regressions.is_empty(),
        "Fields read wrong that are not known failures:\n{}",
        regressions.join("\n")
    );
    assert!(
        unexpected_passes.is_empty(),
        "Known failures that are now read correctly — remove them from known_failures and \
         regenerate the accuracy baseline (UPDATE_SNAPSHOTS=1):\n{}",
        unexpected_passes.join("\n")
    );

    let baseline: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&baseline_path).unwrap_or_else(|e| {
            panic!("{}: {} (run with UPDATE_SNAPSHOTS=1)", ACCURACY_BASELINE, e)
        }),
    )
    .unwrap();
    let base_correct = baseline["correct"].as_u64().unwrap() as usize;
    let base_total = baseline["total"].as_u64().unwrap() as usize;
    assert!(
        overall.correct >= base_correct,
        "Vision accuracy dropped: {}/{} correct fields, baseline {}/{}",
        overall.correct,
        overall.total,
        base_correct,
        base_total
    );
    assert_eq!(
        overall.total, base_total,
        "Fixture set changed; regenerate {} with UPDATE_SNAPSHOTS=1",
        ACCURACY_BASELINE
    );
}
