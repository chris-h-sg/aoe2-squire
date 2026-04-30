use rts_analyzer::{capture, pipeline, replay, types};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let ui_map: types::UiMap = {
        let data = std::fs::read_to_string("ui_map.json").expect("ui_map.json not found");
        serde_json::from_str(&data).expect("failed to parse ui_map.json")
    };
    println!("Loaded {} UI elements", ui_map.elements.len());

    let templates_dir = Path::new("research/templates/enormous_numbers");
    let templates = pipeline::matcher::load_templates(templates_dir);
    println!("Loaded {} templates", templates.len());

    if args.len() > 1 && args[1] == "--parse-replay" {
        let replay_path = Path::new(&args[2]);
        println!("Parsing replay: {}", replay_path.display());
        replay::extract_events(replay_path).expect("Failed to parse replay");
        return;
    }

    if args.len() > 1 && args[1] == "--extract-techs" {
        let replay_path = Path::new(&args[2]);
        replay::extract_events(replay_path).expect("Failed to extract techs");
        return;
    }

    if args.len() > 1 && args[1] != "--live" {
        let image_path = &args[1];
        let img = image::open(image_path)
            .unwrap_or_else(|_| panic!("failed to load image: {}", image_path));
        println!("Image: {}x{}", img.width(), img.height());

        match pipeline::process_frame(&img, &ui_map, &templates) {
            Some(results) => {
                let mut keys: Vec<_> = results.keys().collect();
                keys.sort();
                for cat in keys {
                    let mut sub: Vec<_> = results[cat].iter().collect();
                    sub.sort_by_key(|(k, _)| *k);
                    for (key, val) in sub {
                        println!("  {}_{}: {}", cat, key, val);
                    }
                }
            }
            None => eprintln!(
                "[no anchor] Red UI reference not found — game not visible or UI changed."
            ),
        }
    } else {
        println!("Starting live screen capture...");
        capture::run_capture_loop(&ui_map, &templates).expect("Capture loop failed");
    }
}
