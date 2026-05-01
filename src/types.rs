use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct UiElement {
    pub x_px: f64,
    pub y_px: f64,
    pub w_px: f64,
    pub h_px: f64,
    // split_pct exists in the JSON but is unused by the pipeline
    #[allow(dead_code)]
    pub split_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UiMap {
    pub baseline_margin: f64,
    pub elements: HashMap<String, UiElement>,
}

/// char -> flat f32 array of size CANVAS_SIZE*CANVAS_SIZE, values in [0.0, 1.0]
pub type Templates = HashMap<char, Vec<f32>>;

/// category (e.g. "wood") -> sub_key (e.g. "total") -> digit string (e.g. "423")
pub type Results = HashMap<String, HashMap<String, String>>;

#[derive(Debug, Deserialize, Clone)]
pub struct CsvRow {
    pub timestamp_ms: u64,
    pub food: Option<u32>,
    pub wood: Option<u32>,
    pub gold: Option<u32>,
    pub stone: Option<u32>,
    pub pop_curr: Option<u32>,
    pub pop_max: Option<u32>,
    pub idle_vils: Option<u32>,
}
