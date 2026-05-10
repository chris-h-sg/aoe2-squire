// --- Capture Loop ---
pub const CAPTURE_INTERVAL_MS: u64 = 250;
pub const HOUSED_INTERPOLATION_TIMEOUT_MS: u64 = 1000;
pub const QUEUED_INTERPOLATION_TIMEOUT_MS: u64 = 2000;

// --- Anchor detection (image crate uses RGB, not BGR) ---
pub const RED_R_MIN: u8 = 160;
pub const RED_G_MAX: u8 = 70;
pub const RED_B_MAX: u8 = 70;
pub const RED_PIXEL_MIN_COUNT: usize = 12;

// Spatial boundaries for anchor detection (fraction of frame)
pub const ANCHOR_MIN_X: f64 = 0.5;
pub const ANCHOR_MAX_X: f64 = 0.95;
pub const ANCHOR_MIN_Y: f64 = 0.015;
pub const ANCHOR_MAX_Y: f64 = 0.035;

pub const UI_SCALE_MIN: f64 = 0.5;
pub const UI_SCALE_MAX: f64 = 2.0;

// --- Output / cleanup filter ---
pub const OUT_GREY_TOLERANCE: i16 = 30;
pub const OUT_BRIGHTNESS_THRESHOLD: u8 = 5;
pub const OUT_OVERLAY_BRIGHTNESS_THRESHOLD: u8 = 30;

// --- Segmentation filter ---
pub const SEG_GREY_TOLERANCE: i16 = 30;
pub const SEG_BRIGHTNESS_THRESHOLD: u8 = 100;
pub const SEG_REQUIRED_BRIGHTNESS: u8 = 230;
pub const BASELINE_MIN_AREA: f64 = 15.0;

// --- Canvas / matching ---
pub const WORKING_HEIGHT: u32 = 36;
pub const CANVAS_SIZE: u32 = 64;
pub const BLUR_SIGMA: f32 = 1.0;
pub const WIGGLE_OFFSETS: [i32; 3] = [-1, 0, 1];

// --- Yellow detection ---
pub const YELLOW_MIN_BRIGHTNESS: u8 = 100;
pub const YELLOW_BLUE_MARGIN: i16 = 50;
pub const YELLOW_RG_SIMILARITY: i16 = 50;

// --- Analysis ---
pub const GAME_AGES: [&str; 4] = ["Dark Age", "Feudal Age", "Castle Age", "Imperial Age"];
pub const DEFAULT_MERGED_CSV: &str = "output/merged_observations.csv";

// --- Floating Resources Analysis ---
// See DECISION_LOG.md "Floating Resources Analysis" for threshold rationale.

pub const FLOATING_MIN_DURATION_MS: u64 = 30_000;

/// Duration before a major spend event (age-up click, castle placement) during which
/// the relevant resource accumulation is not flagged as floating (save-up window suppression).
pub const SAVE_UP_WINDOW_MS: u64 = 60_000;

/// Indexed to match GAME_AGES order: Dark Age, Feudal Age, Castle Age, Imperial Age.
pub struct ResourceThresholds {
    pub food: u32,
    pub wood: u32,
    pub gold: u32,
    pub stone: u32,
}

pub const FLOATING_THRESHOLDS: [ResourceThresholds; 4] = [
    ResourceThresholds {
        food: 200,
        wood: 200,
        gold: 100,
        stone: 200,
    }, // Dark Age
    ResourceThresholds {
        food: 500,
        wood: 500,
        gold: 300,
        stone: 300,
    }, // Feudal Age
    ResourceThresholds {
        food: 800,
        wood: 800,
        gold: 500,
        stone: 500,
    }, // Castle Age
    ResourceThresholds {
        food: 1000,
        wood: 1000,
        gold: 1000,
        stone: 1000,
    }, // Imperial Age
];

// --- Smoothing ---
pub const SMOOTH_VILS_MIN_SPIKE: i32 = 2;
pub const SMOOTH_VILS_MAX_DURATION_MS: u64 = 2000;
