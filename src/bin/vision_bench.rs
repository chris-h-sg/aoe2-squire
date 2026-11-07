//! Time per frame of both digit matchers on the vision fixtures (release build, one thread):
//!
//! cargo run --release --bin vision_bench [-- --csv | --dump]
//! cargo run --release --bin vision_bench -- --frames <recorded frames dir>
//!
//! Each frame is processed `REPS` times. For the learned matcher the reader first sees the frame
//! until its shape-set detection is over (as on the first frames of a match), so the time is the
//! steady state of a running match; templates are built once per UI size and are not counted.
//! The mean is over the 1920x1080 frames on which a game is detected.
//!
//! `--frames` instead runs a recording (`--record-frames`) in order, as the capture loop does,
//! with the split matcher and with the learned one reading every field or reusing the readings
//! of unchanged fields; it also counts frames whose results differ between the last two.

use aoe2_squire::pipeline::{
    self, anchor, learned::layout::Field, learned::LearnedReader, matcher,
};
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
    // --dump: every field's reading (text, RMSE, confidence) per file, for checking that a speed
    // change reads exactly the same
    let dump = std::env::args().any(|a| a == "--dump");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_map: UiMap =
        serde_json::from_str(&std::fs::read_to_string(root.join("ui_map.json")).unwrap()).unwrap();
    let templates = matcher::load_embedded_templates();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--frames") {
        frames(std::path::Path::new(&args[i + 1]), &ui_map, &templates);
        return;
    }

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
    let mut field_sums = [[0f64; 5]; 11];
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
        reader.field_ms = [[0.0; 5]; 11];
        let t = Instant::now();
        for _ in 0..REPS {
            std::hint::black_box(reader.process_frame(&img, &ui_map));
        }
        let learned_ms = ms(t);
        let stages = reader.stage_ms.map(|v| v / REPS as f64);
        let field_ms = reader.field_ms;
        if dump {
            let ui_scale = scale.unwrap();
            let u = aoe2_squire::pipeline::learned::layout::px_per_unit(ui_scale);
            let strip = img
                .crop_imm(
                    0,
                    0,
                    ((1140.0 * u).ceil() as u32).min(img.width()),
                    ((106.0 * u).ceil() as u32).min(img.height()),
                )
                .to_rgb8();
            let si = reader.set_index().unwrap();
            for r in reader.read_fields(si, &strip, ui_scale) {
                let (text, rmse, conf) = r
                    .reading
                    .as_ref()
                    .map(|x| (x.text.clone(), x.rmse, x.confidence()))
                    .unwrap_or_default();
                println!(
                    "DUMP {} {} {} {:.4} {:.4}",
                    path.file_name().unwrap().to_string_lossy(),
                    r.field.name(),
                    text,
                    rmse,
                    conf
                );
            }
        }
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
            for (fs, fv) in field_sums.iter_mut().zip(field_ms) {
                for (s, v) in fs.iter_mut().zip(fv) {
                    *s += v / REPS as f64;
                }
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
    println!("Per field, ms per frame: observation / pen height / chains / re-scoring / confidence = total");
    for f in Field::ALL {
        let v = field_sums[f as usize].map(|v| v / n.max(1) as f64);
        println!(
            "  {:<17} {:5.2} / {:5.2} / {:5.2} / {:5.2} / {:5.2} = {:5.2}",
            f.name(),
            v[0],
            v[1],
            v[2],
            v[3],
            v[4],
            v.iter().sum::<f64>()
        );
    }
}

/// Times a recorded match in order (see the module comment).
fn frames(dir: &std::path::Path, ui_map: &UiMap, templates: &aoe2_squire::types::Templates) {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    paths.sort(); // <timestamp_ms>.png
    let imgs: Vec<DynamicImage> = paths
        .iter()
        .map(|p| DynamicImage::ImageRgb8(image::open(p).unwrap().into_rgb8()))
        .collect();
    let n = imgs.len().max(1) as f64;
    let run = |matcher: pipeline::Matcher, skip: bool| {
        let mut reader = pipeline::PanelReader::new(matcher, ui_map, templates);
        reader.skip_unchanged(skip);
        let t = Instant::now();
        let out: Vec<_> = imgs.iter().map(|img| reader.process_frame(img)).collect();
        (t.elapsed().as_secs_f64() * 1000.0 / n, out, reader)
    };
    let (split_ms, _, _) = run(pipeline::Matcher::Split, false);
    let (full_ms, full, _) = run(pipeline::Matcher::Learned, false);
    let (skip_ms, skipped, reader) = run(pipeline::Matcher::Learned, true);
    let (read, reused) = match &reader {
        pipeline::PanelReader::Learned { reader, .. } => (reader.fields_read, reader.fields_reused),
        _ => (0, 0),
    };
    let differ = full.iter().zip(&skipped).filter(|(a, b)| a != b).count();
    println!(
        "{} frames of {}: split matcher {:.2} ms/frame, learned matcher {:.2} ms/frame reading every field, {:.2} ms/frame reusing unchanged fields ({} fields read, {} reused); frames whose results differ between the last two: {}",
        imgs.len(),
        dir.display(),
        split_ms,
        full_ms,
        skip_ms,
        read,
        reused,
        differ
    );
}
