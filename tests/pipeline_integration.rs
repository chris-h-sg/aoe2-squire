use aoe2_squire::{pipeline, types};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct ExpectedJson {
    value_sets: HashMap<String, HashMap<String, HashMap<String, String>>>,
}

struct Fixtures {
    ui_map: types::UiMap,
    templates: types::Templates,
    expected: ExpectedJson,
}

// Safety: all contained types are Send + Sync (HashMap, String, f64, Vec<f32>).
unsafe impl Send for Fixtures {}
unsafe impl Sync for Fixtures {}

static FIXTURES: OnceLock<Fixtures> = OnceLock::new();

fn get_fixtures() -> &'static Fixtures {
    FIXTURES.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));

        let ui_map_data =
            std::fs::read_to_string(root.join("ui_map.json")).expect("ui_map.json not found");
        let ui_map: types::UiMap =
            serde_json::from_str(&ui_map_data).expect("failed to parse ui_map.json");

        let templates =
            pipeline::matcher::load_templates(&root.join("research/templates/enormous_numbers"));

        let expected_data = std::fs::read_to_string(root.join("test_bench/expected_values.json"))
            .expect("expected_values.json not found");
        let expected: ExpectedJson =
            serde_json::from_str(&expected_data).expect("failed to parse expected_values.json");

        Fixtures {
            ui_map,
            templates,
            expected,
        }
    })
}

fn run_image_test(img_name: &str, value_set_name: &str) {
    let f = get_fixtures();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let img = image::open(root.join("test_bench").join(img_name))
        .unwrap_or_else(|e| panic!("failed to load {}: {}", img_name, e));

    let results = pipeline::process_frame(&img, &f.ui_map, &f.templates)
        .unwrap_or_else(|| panic!("no UI anchor detected in {}", img_name));

    let expected_values = f
        .expected
        .value_sets
        .get(value_set_name)
        .unwrap_or_else(|| panic!("value set '{}' not found", value_set_name));

    let mut failures = vec![];
    for (category, sub_map) in expected_values {
        for (sub_key, expected_val) in sub_map {
            let actual = results
                .get(category)
                .and_then(|m| m.get(sub_key))
                .map(String::as_str)
                .unwrap_or("");
            if actual != expected_val.as_str() {
                failures.push(format!(
                    "  {}_{}: got {:?}, expected {:?}",
                    category, sub_key, actual, expected_val
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Mismatches in {}:\n{}",
        img_name,
        failures.join("\n")
    );
}

macro_rules! image_test {
    ($fn_name:ident, $img:expr, $set:expr) => {
        #[test]
        fn $fn_name() {
            run_image_test($img, $set);
        }
    };
}

image_test!(aoe2_16x9, "aoe2_16x9.png", "baseline_1080p");
image_test!(aoe2_16x9_min, "aoe2_16x9_min.png", "baseline_1080p");
image_test!(aoe2_16x9_max, "aoe2_16x9_max.png", "baseline_1080p");
image_test!(aoe2_16x10, "aoe2_16x10.png", "baseline_other");
image_test!(aoe2_21x9, "aoe2_21x9.png", "baseline_other");
image_test!(aoe2_32x9, "aoe2_32x9.png", "baseline_other");
image_test!(aoe2_4k, "aoe2_4k.png", "baseline_other");
image_test!(housed_overlay, "housed_overlay.png", "housed_overlay");
image_test!(housed_no_overlay, "housed_no_overlay.png", "housed_no_overlay");

// Anne_HK Mod Tests
image_test!(aoe2_16x9_anne_hk, "aoe2_16x9_Anne_HK.png", "baseline_1080p");
image_test!(aoe2_16x9_anne_hk_no_idle, "aoe2_16x9_Anne_HK-no-idle.png", "baseline_1080p_no_idle");
image_test!(aoe2_16x9_anne_hk_103_idle, "aoe2_16x9_Anne_HK-103-idle.png", "baseline_1080p_103_idle");
image_test!(aoe2_1366x768_anne_hk, "aoe2_1366x768_Anne_HK.png", "Anne_HK_1366x768");
