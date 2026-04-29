// --- Anchor detection (image crate uses RGB, not BGR) ---
pub const RED_R_MIN: u8 = 201;
pub const RED_G_MAX: u8 = 60;
pub const RED_B_MAX: u8 = 60;
pub const RED_PIXEL_MIN_COUNT: usize = 5;
/// Fraction of frame height to scan for the red anchor pixels.
/// Restricted to the top 5% to avoid stray red pixels from taskbar icons.
pub const ANCHOR_SCAN_FRACTION: f64 = 0.05;

// --- Output / cleanup filter ---
pub const OUT_GREY_TOLERANCE: i16 = 20;
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
