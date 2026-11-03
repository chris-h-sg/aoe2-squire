//! Time per frame of both digit matchers on the vision fixtures (release build, one thread):
//!
//! cargo run --release --bin vision_bench [-- --csv]
//!
//! Each frame is processed `REPS` times. For the learned matcher the reader first sees the frame
//! until its shape-set detection is over (as on the first frames of a match), so the time is the
//! steady state of a running match; templates are built once per UI size and are not counted.
//! The mean is over the 1920x1080 frames on which a game is detected.

use aoe2_squire::pipeline::{self, anchor, learned::LearnedReader, matcher};
use aoe2_squire::types::UiMap;
use image::DynamicImage;
use std::time::Instant;

const REPS: usize = 5;
const DIRS: [&str; 5] = [
    "test_bench",
    "test_bench/video_frames",
    "test_bench/game_frames/game1-castle-vs-ai",
    "test_bench/game_frames/game2-fast-pauses",
    "test_bench/game_frames/game3-playback-75ui",
];

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0 / REPS as f64
}

fn main() {
    let csv = std::env::args().any(|a| a == "--csv");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_map: UiMap =
        serde_json::from_str(&std::fs::read_to_string(root.join("ui_map.json")).unwrap()).unwrap();
    let templates = matcher::load_embedded_templates();

    let mut files = vec![];
    for dir in DIRS {
        for e in std::fs::read_dir(root.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "png") {
                files.push(p);
            }
        }
    }
    if csv {
        println!("file,WxH,anchor_ms,split_ms,learned_ms,shape_set");
    }
    let (mut n, mut sums, mut stage_sums) = (0usize, [0f64; 3], [0f64; 5]);
    for path in &files {
        let img = DynamicImage::ImageRgb8(image::open(path).unwrap().into_rgb8());
        let t = Instant::now();
        let mut scale = None;
        for _ in 0..REPS {
            scale = anchor::detect_ui_scale(&img, ui_map.baseline_margin);
        }
        let anchor_ms = ms(t);
        if scale.is_none() {
            continue;
        }
        let t = Instant::now();
        for _ in 0..REPS {
            std::hint::black_box(pipeline::process_frame(&img, &ui_map, &templates));
        }
        let split_ms = ms(t);
        let mut reader = LearnedReader::new();
        while reader.set_id().is_none() {
            reader.process_frame(&img, &ui_map);
        }
        reader.stage_ms = [0.0; 5];
        let t = Instant::now();
        for _ in 0..REPS {
            std::hint::black_box(reader.process_frame(&img, &ui_map));
        }
        let learned_ms = ms(t);
        let stages = reader.stage_ms.map(|v| v / REPS as f64);
        if csv {
            println!(
                "{},{}x{},{:.2},{:.2},{:.2},{}",
                path.file_name().unwrap().to_string_lossy(),
                img.width(),
                img.height(),
                anchor_ms,
                split_ms,
                learned_ms,
                reader.set_id().unwrap_or("")
            );
        }
        if img.width() == 1920 && img.height() == 1080 {
            n += 1;
            for (s, v) in sums.iter_mut().zip([anchor_ms, split_ms, learned_ms]) {
                *s += v;
            }
            for (s, v) in stage_sums.iter_mut().zip(stages) {
                *s += v;
            }
        }
    }
    let m = |i: usize| sums[i] / n.max(1) as f64;
    println!(
        "Mean over {} detected 1920x1080 frames: split matcher {:.2} ms/frame, learned matcher {:.2} ms/frame (anchor search, in both, {:.2} ms)",
        n,
        m(1),
        m(2),
        m(0)
    );
    let st = stage_sums.map(|v| v / n.max(1) as f64);
    println!(
        "Learned matcher per frame by stage: observation {:.2} ms, pen height {:.2}, chains {:.2}, re-scoring {:.2}, confidence {:.2} (rest: anchor, strip copy, results {:.2})",
        st[0],
        st[1],
        st[2],
        st[3],
        st[4],
        m(2) - st.iter().sum::<f64>()
    );
}
