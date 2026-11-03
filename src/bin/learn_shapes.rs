//! Learns the glyph shape sets of the learned-shape matcher from screenshots with known values
//! (`data/digit_shapes/training.json`) and writes them to `data/digit_shapes/`.
//!
//! cargo run --release --bin learn_shapes -- --game-dir <AoE2DE install> [--only <set id>]
//!
//! The game directory provides the font metrics and atlas the shapes start from
//! (`resources/_common/fonts/<font>.txt` and `<font>_<page>.png`). See
//! `data/digit_shapes/README.md`.

use aoe2_squire::pipeline::anchor;
use aoe2_squire::pipeline::learned::layout::{px_per_unit, Field, Kind, Layout};
use aoe2_squire::pipeline::learned::observe::{observe, text_colour};
use aoe2_squire::pipeline::learned::shapes::ShapeSet;
use aoe2_squire::pipeline::learned::train::{self, Sample};
use aoe2_squire::types::UiMap;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Manifest {
    value_sets: HashMap<String, HashMap<String, String>>,
    sets: Vec<SetSpec>,
}

#[derive(Deserialize)]
struct SetSpec {
    id: String,
    layout: String,
    font: String,
    description: String,
    shots: Vec<(String, String)>,
}

const ROUNDS: usize = 3;
const ITERS: usize = 600;
const LAMBDA: f32 = 0.002;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let Some(game_dir) = value("--game-dir") else {
        eprintln!(
            "Usage: learn_shapes --game-dir <AoE2DE install dir> [--only <set id>] [--out <dir>]"
        );
        std::process::exit(2);
    };
    let fonts = Path::new(&game_dir).join("resources/_common/fonts");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = value("--out")
        .map(PathBuf::from)
        .unwrap_or(root.join("data/digit_shapes"));
    let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(
        root.join("data/digit_shapes/training.json"),
    )?)?;
    let ui_map: UiMap = serde_json::from_str(&std::fs::read_to_string(root.join("ui_map.json"))?)?;

    for spec in &manifest.sets {
        if value("--only").is_some_and(|only| only != spec.id) {
            continue;
        }
        println!(
            "== {} ({} screenshots, font {}, layout {})",
            spec.id,
            spec.shots.len(),
            spec.font,
            spec.layout
        );
        let layout =
            Layout::by_name(&spec.layout).ok_or(format!("unknown layout {}", spec.layout))?;
        let metrics = train::load_font(&fonts, &spec.font)?;
        let init = train::atlas_shapes(&fonts, &spec.font, &metrics)?;

        let mut samples: Vec<Sample> = vec![];
        for (shot, set_name) in &spec.shots {
            let values = manifest
                .value_sets
                .get(set_name)
                .ok_or(format!("unknown value set {set_name}"))?;
            let img = image::open(root.join(shot)).map_err(|e| format!("{shot}: {e}"))?;
            let ui_scale = anchor::detect_ui_scale(&img, ui_map.baseline_margin)
                .ok_or(format!("{shot}: panel not found"))?;
            let u = px_per_unit(ui_scale);
            let rgb = img.to_rgb8();
            for (name, text) in values {
                let field = Field::from_name(name).ok_or(format!("unknown field {name}"))?;
                let fl = layout.field(field);
                let colour = if field == Field::PopTotal {
                    text_colour(&rgb, (0, 0), fl, u).rgb()
                } else {
                    fl.colour
                };
                let obs = observe(&rgb, (0, 0), fl, u, colour, Some(text.chars().count()))
                    .ok_or(format!("{shot}: {name} outside the frame"))?;
                samples.push(Sample {
                    field,
                    text: text.clone(),
                    obs,
                    pens: vec![],
                });
            }
            println!("  {shot}: ui_scale {ui_scale:.4}");
        }

        let (mut counts, mut totals): (Vec<Sample>, Vec<Sample>) = samples
            .into_iter()
            .partition(|s| s.field.kind() == Kind::Count);
        let log = |s: &str| println!("{s}");
        println!(" counts ({} samples)", counts.len());
        let mut count = train::learn_kind(&mut counts, init.clone(), ROUNDS, ITERS, LAMBDA, log);
        println!(" totals ({} samples)", totals.len());
        let mut total = train::learn_kind(&mut totals, init, ROUNDS, ITERS, LAMBDA, log);
        for (samples, set) in [(&counts, &count), (&totals, &total)] {
            let per: Vec<String> = train::ink_rmse_by_field(samples, set)
                .iter()
                .map(|(f, r)| format!("{} {:.1}", f.name(), r))
                .collect();
            println!(" RMSE per field: {}", per.join(", "));
        }
        train::learn_advances(&counts, &mut count);
        train::learn_advances(&totals, &mut total);
        let (cs, ts) = (train::seen(&counts), train::seen(&totals));
        let (count0, total0) = (count.clone(), total.clone());
        let to_count = train::fill_unseen(&mut count, &cs, &total0, &ts);
        let to_total = train::fill_unseen(&mut total, &ts, &count0, &cs);
        println!(" counts seen {cs}, from totals {to_count:?}; totals seen {ts}, from counts {to_total:?}");
        let mut pen_y = train::pen_heights(&counts);
        pen_y.extend(train::pen_heights(&totals));
        let set = ShapeSet {
            id: spec.id.clone(),
            layout,
            count,
            total,
            pen_y,
        };
        set.save(&out, &spec.description)?;
        println!(
            " written {}",
            out.join(format!("{}.json", spec.id)).display()
        );
    }
    Ok(())
}
