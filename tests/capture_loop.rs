//! Runs the capture loop (`capture::capture_match`) on frames from test_bench/video_frames
//! through `ImageSequenceSource`, without a game: game start/end detection, telemetry rows
//! (checked against the fixture manifest), housing interpolation, and recording frames
//! with `--record-frames` and replaying them.
//!
//! Frame timestamps come from the `timestamp_sec` column of the test_bench expected CSVs.
//! Black frames stand in for "no match on screen".
mod common;

use aoe2_squire::capture::record::{mask_panel, save_png, RecordOptions};
use aoe2_squire::capture::source::{ImageSequenceSource, FRAMES_INDEX};
use aoe2_squire::capture::{capture_match, CaptureOptions, CapturedMatch, TELEMETRY_HEADER};
use aoe2_squire::{pipeline, types};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const BASE_MS: u64 = 1_700_000_000_000;

/// Manifest field -> telemetry CSV column.
const FIELD_COLUMNS: [(&str, &str); 12] = [
    ("food.total", "food_total"),
    ("food.vils", "food_vils"),
    ("wood.total", "wood_total"),
    ("wood.vils", "wood_vils"),
    ("gold.total", "gold_total"),
    ("gold.vils", "gold_vils"),
    ("stone.total", "stone_total"),
    ("stone.vils", "stone_vils"),
    ("population.total", "pop_curr"),
    ("population.housing", "pop_max"),
    ("population.vils", "pop_vils"),
    ("idle.vils", "idle_vils"),
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("squire_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct TestFrame {
    path: PathBuf,
    timestamp_ms: u64,
    /// Expected telemetry columns (only fields the pipeline reads correctly today).
    expected: BTreeMap<&'static str, String>,
    /// Expected `housing` column; None for black frames.
    housing: Option<String>,
}

fn ui_map() -> types::UiMap {
    serde_json::from_str(include_str!("../ui_map.json")).unwrap()
}

fn run(frames: &[TestFrame], opts: &CaptureOptions) -> Option<CapturedMatch> {
    let mut source = ImageSequenceSource::new(
        frames
            .iter()
            .map(|f| (f.path.clone(), f.timestamp_ms))
            .collect(),
    );
    let templates = pipeline::matcher::load_embedded_templates();
    capture_match(&mut source, &ui_map(), &templates, opts).unwrap()
}

fn options(log_dir: &Path) -> CaptureOptions {
    CaptureOptions {
        log_dir: log_dir.to_path_buf(),
        ..Default::default()
    }
}

/// The frames of one video listed in a test_bench expected CSV, with their expected values
/// from test_bench/video_frames/expected_values.json minus its known failures.
fn video_frames(expected_csv: &str, stem: &str) -> Vec<TestFrame> {
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(common::test_bench("video_frames/expected_values.json")).unwrap(),
    )
    .unwrap();
    let mut rdr = csv::Reader::from_path(common::test_bench(expected_csv)).unwrap();
    rdr.deserialize::<HashMap<String, String>>()
        .map(|row| {
            let row = row.unwrap();
            let name = format!(
                "{}_f{:04}.png",
                stem,
                row["frame_idx"].parse::<u32>().unwrap()
            );
            let entry = &manifest["images"][&name];
            let known_failures = entry["known_failures"].as_object();
            let mut expected = BTreeMap::new();
            for (field, column) in FIELD_COLUMNS {
                if known_failures.is_some_and(|k| k.contains_key(field) || k.contains_key("*")) {
                    continue;
                }
                let (cat, sub) = field.split_once('.').unwrap();
                expected.insert(
                    column,
                    entry["values"][cat][sub].as_str().unwrap().to_string(),
                );
            }
            let seconds: f64 = row["timestamp_sec"].parse().unwrap();
            TestFrame {
                path: common::test_bench("video_frames").join(&name),
                timestamp_ms: BASE_MS + (seconds * 1000.0).round() as u64,
                expected,
                housing: Some(row["housing"].clone()),
            }
        })
        .collect()
}

fn black_frame(dir: &Path, like: &Path, timestamp_ms: u64) -> TestFrame {
    let path = dir.join("black.png");
    if !path.exists() {
        let (w, h) = image::image_dimensions(like).unwrap();
        image::RgbImage::new(w, h).save(&path).unwrap();
    }
    TestFrame {
        path,
        timestamp_ms,
        expected: BTreeMap::new(),
        housing: None,
    }
}

/// `video` framed by two black frames before (no match yet) and one after (match over).
fn with_black_frames(dir: &Path, video: Vec<TestFrame>) -> Vec<TestFrame> {
    let (first, last) = (video[0].timestamp_ms, video.last().unwrap().timestamp_ms);
    let like = video[0].path.clone();
    let mut frames = vec![
        black_frame(dir, &like, first - 500),
        black_frame(dir, &like, first - 250),
    ];
    frames.extend(video);
    frames.push(black_frame(dir, &like, last + 250));
    frames
}

fn read_rows(path: &Path) -> Vec<HashMap<String, String>> {
    let mut rdr = csv::Reader::from_path(path).unwrap();
    assert_eq!(
        rdr.headers().unwrap().iter().collect::<Vec<_>>(),
        TELEMETRY_HEADER
    );
    rdr.deserialize().map(|r| r.unwrap()).collect()
}

/// Checks one telemetry row per frame (timestamp, every expected field and housing).
fn check_rows(rows: &[HashMap<String, String>], frames: &[&TestFrame]) {
    let mut errors = vec![];
    let timestamps = |v: Vec<u64>| v;
    assert_eq!(
        timestamps(
            rows.iter()
                .map(|r| r["timestamp_ms"].parse().unwrap())
                .collect()
        ),
        frames.iter().map(|f| f.timestamp_ms).collect::<Vec<_>>(),
        "one row per frame after the start frame, stamped with the frame's capture time"
    );
    for (row, frame) in rows.iter().zip(frames) {
        let name = frame.path.file_name().unwrap().to_string_lossy();
        for (column, value) in &frame.expected {
            if &row[*column] != value {
                errors.push(format!(
                    "{} {}: expected {:?}, got {:?}",
                    name, column, value, row[*column]
                ));
            }
        }
        if let Some(housing) = &frame.housing {
            if &row["housing"] != housing {
                errors.push(format!(
                    "{} housing: expected {:?}, got {:?}",
                    name, housing, row["housing"]
                ));
            }
        }
    }
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}

/// Runs `video` between black frames and checks start/end detection and every row.
fn check_match(name: &str, video: Vec<TestFrame>) {
    let tmp = TempDir::new(name);
    let frames = with_black_frames(&tmp.0, video);
    let captured = run(&frames, &options(&tmp.0)).expect("match detected");

    // The first frame with every field readable starts the match; it writes no row.
    let start = &frames[2];
    assert_eq!(
        captured.session_start,
        UNIX_EPOCH + Duration::from_millis(start.timestamp_ms)
    );
    assert!(captured.game_ended, "the black frame ends the match");
    assert!(captured
        .telemetry
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("telemetry_"));

    let rows = read_rows(&captured.telemetry);
    check_rows(
        &rows,
        &frames[3..frames.len() - 1].iter().collect::<Vec<_>>(),
    );
}

#[test]
fn extract_video_rows_match_expected_values() {
    check_match(
        "extract",
        video_frames("extract_expected_1s.csv", "extract"),
    );
}

#[test]
fn extract2_video_rows_match_expected_values() {
    check_match(
        "extract2",
        video_frames("extract2_expected_1s.csv", "extract2"),
    );
}

/// Overlay / yellow / white population at 0.5 s steps: exercises the housed and queued
/// bridges, including the frame still buffered by the interpolation when the match ends.
#[test]
fn housed_video_housing_is_interpolated() {
    check_match(
        "housed",
        video_frames("extract-housed_expected_0.5s.csv", "extract-housed"),
    );
}

/// The match ends right after a white frame inside a housed bridge (the overlay last
/// showed at 8.5 s): the frame keeps the housed state instead of falling back to normal.
#[test]
fn match_ending_mid_bridge_keeps_housing_state() {
    let mut video = video_frames("extract-housed_expected_0.5s.csv", "extract-housed");
    video.retain(|f| f.timestamp_ms <= BASE_MS + 9000);
    assert_eq!(video.last().unwrap().housing.as_deref(), Some("housed"));
    check_match("mid_bridge", video);
}

#[test]
fn no_match_on_screen_records_nothing() {
    let tmp = TempDir::new("no_match");
    let like = common::test_bench("video_frames/extract_f0000.png");
    let frames: Vec<_> = (0..3)
        .map(|i| black_frame(&tmp.0, &like, BASE_MS + i * 250))
        .collect();
    assert!(run(&frames, &options(&tmp.0)).is_none());
    let csvs = std::fs::read_dir(&tmp.0)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "csv")
        })
        .count();
    assert_eq!(csvs, 0);
}

#[test]
fn source_running_out_mid_match_keeps_all_rows() {
    let tmp = TempDir::new("exhausted");
    let video = video_frames("extract-housed_expected_0.5s.csv", "extract-housed");
    let captured = run(&video, &options(&tmp.0)).expect("match detected");
    assert!(!captured.game_ended);
    let rows = read_rows(&captured.telemetry);
    check_rows(&rows, &video[1..].iter().collect::<Vec<_>>());
}

#[test]
fn image_sequence_from_dir() {
    let tmp = TempDir::new("from_dir");
    let names = [
        "extract_f0061.png",
        "extract_f0000.png",
        "extract_f0031.png",
    ];
    let stamps = [BASE_MS + 2000, BASE_MS, BASE_MS + 1000];

    // <timestamp_ms>.png file names
    let by_name = tmp.0.join("by_name");
    std::fs::create_dir_all(&by_name).unwrap();
    for (name, ts) in names.iter().zip(stamps) {
        let src = common::test_bench("video_frames").join(name);
        std::fs::copy(src, by_name.join(format!("{}.png", ts))).unwrap();
    }
    std::fs::write(by_name.join("notes.txt"), "ignored").unwrap();

    // frames.csv index with arbitrary file names
    let indexed = tmp.0.join("indexed");
    std::fs::create_dir_all(&indexed).unwrap();
    let mut index = String::from("timestamp_ms,file,reason\n");
    for (name, ts) in names.iter().zip(stamps) {
        std::fs::copy(
            common::test_bench("video_frames").join(name),
            indexed.join(name),
        )
        .unwrap();
        index.push_str(&format!("{},{},test\n", ts, name));
    }
    std::fs::write(indexed.join(FRAMES_INDEX), index).unwrap();

    for dir in [by_name, indexed] {
        let mut source = ImageSequenceSource::from_dir(&dir).unwrap();
        assert_eq!(source.remaining(), 3);
        let templates = pipeline::matcher::load_embedded_templates();
        let opts = options(&dir.join("out"));
        let captured = capture_match(&mut source, &ui_map(), &templates, &opts)
            .unwrap()
            .unwrap();
        let rows = read_rows(&captured.telemetry);
        // Sorted by timestamp: f0000 starts the match, f0031 and f0061 write rows.
        let wood: Vec<_> = rows.iter().map(|r| r["wood_total"].as_str()).collect();
        assert_eq!(wood, ["27", "34"], "{}", dir.display());
    }

    let empty = tmp.0.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(ImageSequenceSource::from_dir(&empty).is_err());
}

/// Records the housed video with `--record-frames`, checks what was saved, then replays the
/// saved frames through the capture loop and gets the same readings back.
#[test]
fn recorded_frames_replay_to_the_same_rows() {
    let tmp = TempDir::new("record");
    let video = video_frames("extract-housed_expected_0.5s.csv", "extract-housed");
    let frames = with_black_frames(&tmp.0, video);
    let opts = CaptureOptions {
        log_dir: tmp.0.clone(),
        record_frames: Some(tmp.0.join("frames")),
        record: RecordOptions {
            interval_ms: 2000,
            queue_capacity: 64, // never drop in the test
            ..Default::default()
        },
    };
    let captured = run(&frames, &opts).unwrap();
    let frames_dir = captured.frames_dir.clone().expect("frames recorded");
    assert_eq!(
        frames_dir.file_name(),
        captured.telemetry.file_stem(),
        "the frames directory is named after the telemetry file"
    );
    let rows = read_rows(&captured.telemetry);

    let mut rdr = csv::Reader::from_path(frames_dir.join(FRAMES_INDEX)).unwrap();
    let saved: Vec<(u64, String, String)> = rdr.deserialize().map(|r| r.unwrap()).collect();
    let ms = |s: f64| BASE_MS + (s * 1000.0) as u64;
    let reasons: Vec<(u64, &str)> = saved.iter().map(|(t, _, r)| (*t, r.as_str())).collect();
    assert_eq!(
        reasons,
        [
            (ms(0.5), "first"), // first row (0.0 s started the match)
            (ms(2.5), "interval"),
            (ms(3.5), "change:pop_color"), // housed overlay appears
            (ms(4.0), "before"),
            (ms(4.5), "change:stone_total"), // castle placed: 749 -> 99 stone
            (ms(5.5), "change:pop_color"),   // the overlay blinks every second
            (ms(6.5), "change:pop_color"),
            (ms(7.5), "change:pop_color"),
            (ms(8.5), "change:pop_color"),
            (ms(9.5), "change:pop_color"),
            (ms(10.25), "end"), // the black frame that ended the match
        ]
    );

    // Every saved frame except `end` matches a telemetry row by timestamp, and is the
    // masked source frame (the fixtures are already masked, so pixel-identical).
    let by_ts: HashMap<u64, &TestFrame> = frames.iter().map(|f| (f.timestamp_ms, f)).collect();
    for (ts, file, reason) in &saved {
        assert_eq!(file, &format!("{}.png", ts));
        if reason != "end" {
            assert!(rows.iter().any(|r| r["timestamp_ms"] == ts.to_string()));
        }
        let original = image::open(&by_ts[ts].path).unwrap().into_rgb8();
        let saved_img = image::open(frames_dir.join(file)).unwrap().into_rgb8();
        assert!(saved_img == mask_panel(&original), "{} differs", file);
    }

    // Replaying the saved frames reproduces the readings (housing differs: it is
    // interpolated from the much sparser frames).
    let replay_opts = options(&tmp.0.join("replayed"));
    let mut source = ImageSequenceSource::from_dir(&frames_dir).unwrap();
    let templates = pipeline::matcher::load_embedded_templates();
    let replayed = capture_match(&mut source, &ui_map(), &templates, &replay_opts)
        .unwrap()
        .unwrap();
    assert!(replayed.game_ended);
    let replayed_rows = read_rows(&replayed.telemetry);
    assert_eq!(replayed_rows.len(), saved.len() - 2, "minus start and end");
    for replayed_row in &replayed_rows {
        let original = rows
            .iter()
            .find(|r| r["timestamp_ms"] == replayed_row["timestamp_ms"])
            .unwrap();
        for (_, column) in FIELD_COLUMNS {
            assert_eq!(replayed_row[column], original[column], "{}", column);
        }
    }
}

/// Recording cost per saved frame (masking + PNG encoding + write) on full screenshots,
/// and the file size. Run with `cargo test --release --test capture_loop -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark; prints timings"]
fn recording_overhead() {
    let tmp = TempDir::new("overhead");
    for name in ["aoe2_16x9.png", "aoe2_4k.png"] {
        let img = image::open(common::test_bench(name)).unwrap().into_rgb8();
        let n = 10;
        let start = Instant::now();
        let mut bytes = 0;
        for i in 0..n {
            let path = tmp.0.join(format!("{}.png", i));
            save_png(&mask_panel(&img), &path).unwrap();
            bytes = std::fs::metadata(&path).unwrap().len();
        }
        let per_frame = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "{} {}x{}: {:.1} ms and {} KB per saved frame",
            name,
            img.width(),
            img.height(),
            per_frame,
            bytes / 1024
        );
    }
}
