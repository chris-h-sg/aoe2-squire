mod capture;
mod constants;
mod pipeline;
mod types;

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let image_path = args.get(1).map(String::as_str).unwrap_or("test_bench/aoe2_16x9.png");

    let ui_map: types::UiMap = {
        let data = std::fs::read_to_string("ui_map.json").expect("ui_map.json not found");
        serde_json::from_str(&data).expect("failed to parse ui_map.json")
    };
    println!("Loaded {} UI elements", ui_map.elements.len());

    let templates_dir = Path::new("research/templates/enormous_numbers");
    let templates = pipeline::matcher::load_templates(templates_dir);
    println!("Loaded {} templates", templates.len());

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
        None => eprintln!("[no anchor] Red UI reference not found — game not visible or UI changed."),
    }
}
