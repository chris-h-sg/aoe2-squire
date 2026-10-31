pub mod record;
pub mod source;

use crate::types::{Results, Templates, UiMap};
use chrono::Local;
use record::{FrameRecorder, PanelValues, RecordOptions};
use source::{Frame, FrameSource, SourceEvent};
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::{DynamicImage, RgbImage};
use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_OUTDUPL_FRAME_INFO,
};

use crate::pipeline;

pub fn prepare_telemetry_row(results: &Results, timestamp: u128) -> Vec<String> {
    let get_val = |cat: &str, sub: &str| {
        results
            .get(cat)
            .and_then(|m| m.get(sub).map(|s| s.as_str()))
            .unwrap_or("")
    };

    let pop_curr = get_val("population", "total");
    let pop_max = get_val("population", "housing");

    vec![
        timestamp.to_string(),
        get_val("food", "total").to_string(),
        get_val("food", "vils").to_string(),
        get_val("wood", "total").to_string(),
        get_val("wood", "vils").to_string(),
        get_val("gold", "total").to_string(),
        get_val("gold", "vils").to_string(),
        get_val("stone", "total").to_string(),
        get_val("stone", "vils").to_string(),
        pop_curr.to_string(),
        pop_max.to_string(),
        get_val("population", "vils").to_string(),
        get_val("idle", "vils").to_string(),
        get_val("population", "status").to_string(),
    ]
}

pub struct CaptureSession {
    _device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging_texture: ID3D11Texture2D,
    width: u32,
    height: u32,
}

impl CaptureSession {
    pub fn new() -> Result<Self> {
        // 1. Setup D3D11 Device
        let mut device = None;
        let mut context = None;
        let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];

        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }

        let device: ID3D11Device = device.unwrap();
        let context: ID3D11DeviceContext = context.unwrap();

        // 2. Get DXGI Device -> Adapter -> Output -> OutputDuplication
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi_device.GetAdapter()? };
        let output = unsafe { adapter.EnumOutputs(0)? }; // Primary monitor
        let output1: IDXGIOutput1 = output.cast()?;
        let duplication: IDXGIOutputDuplication = unsafe { output1.DuplicateOutput(&device)? };

        // Get output description for dimensions
        let output_desc = unsafe { output.GetDesc()? };
        let width =
            (output_desc.DesktopCoordinates.right - output_desc.DesktopCoordinates.left) as u32;
        let height =
            (output_desc.DesktopCoordinates.bottom - output_desc.DesktopCoordinates.top) as u32;

        // 3. Create Staging Texture
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging_texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging_texture))? };
        let staging_texture: ID3D11Texture2D = staging_texture.unwrap();

        Ok(Self {
            _device: device,
            context,
            duplication,
            staging_texture,
            width,
            height,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Acquires a single frame from the desktop.
    /// Returns None if no frame is available (e.g. timeout or no screen update).
    pub fn capture_frame(&mut self) -> Result<Option<DynamicImage>> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;

        // Acquire the next frame (timeout 0, we assume the caller handles timing)
        let res = unsafe {
            self.duplication
                .AcquireNextFrame(0, &mut frame_info, &mut desktop_resource)
        };
        if res.is_err() {
            return Ok(None);
        }

        let desktop_resource = desktop_resource.unwrap();
        let desktop_texture: ID3D11Texture2D = desktop_resource.cast()?;

        // Copy from GPU texture to CPU-readable staging texture
        unsafe {
            self.context
                .CopyResource(&self.staging_texture, &desktop_texture);
        }

        // Map the staging texture to read pixels
        let mut mapped: D3D11_MAPPED_SUBRESOURCE = Default::default();
        let image = unsafe {
            if self
                .context
                .Map(
                    &self.staging_texture,
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut mapped),
                )
                .is_ok()
            {
                let ptr = mapped.pData as *const u8;
                let pitch = mapped.RowPitch as usize;
                let data_slice = std::slice::from_raw_parts(ptr, (self.height as usize) * pitch);
                let rgb_image = bgra_to_rgb_image(data_slice, self.width, self.height, pitch);

                self.context.Unmap(&self.staging_texture, 0);
                Some(DynamicImage::ImageRgb8(rgb_image))
            } else {
                None
            }
        };

        unsafe {
            let _ = self.duplication.ReleaseFrame();
        }

        Ok(image)
    }
}

/// Live frame source: DXGI desktop duplication, paced to one capture per `interval`.
pub struct DxgiSource {
    session: CaptureSession,
    interval: Duration,
    next_tick: Instant,
}

impl DxgiSource {
    pub fn new(interval: Duration) -> Result<Self> {
        Ok(Self {
            session: CaptureSession::new()?,
            interval,
            next_tick: Instant::now(),
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.session.dimensions()
    }
}

impl FrameSource for DxgiSource {
    fn next_frame(&mut self) -> BoxResult<SourceEvent> {
        self.next_tick = Instant::now() + self.interval;
        Ok(match self.session.capture_frame()? {
            Some(image) => SourceEvent::Frame(Frame {
                image,
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            }),
            None => SourceEvent::Pending,
        })
    }

    fn end_tick(&mut self) {
        let now = Instant::now();
        if self.next_tick > now {
            std::thread::sleep(self.next_tick - now);
        }
    }
}

type BoxResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(PartialEq)]
enum CaptureState {
    WaitingForGame,
    Recording,
    GameEnded,
}

pub struct CaptureOptions {
    /// Directory for `telemetry_<match start>.csv`.
    pub log_dir: PathBuf,
    /// Save masked frames of the match to `<dir>/telemetry_<match start>/` (see `record`).
    pub record_frames: Option<PathBuf>,
    pub record: RecordOptions,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            record_frames: None,
            record: RecordOptions::default(),
        }
    }
}

pub struct CapturedMatch {
    pub telemetry: PathBuf,
    /// Capture time of the frame on which the match was detected.
    pub session_start: SystemTime,
    /// The match UI disappeared; false if the frame source ran out first.
    pub game_ended: bool,
    pub frames_dir: Option<PathBuf>,
}

pub const TELEMETRY_HEADER: [&str; 14] = [
    "timestamp_ms",
    "food_total",
    "food_vils",
    "wood_total",
    "wood_vils",
    "gold_total",
    "gold_vils",
    "stone_total",
    "stone_vils",
    "pop_curr",
    "pop_max",
    "pop_vils",
    "idle_vils",
    "housing",
];

/// Live capture: waits for a match on the primary monitor, records it, then looks for its
/// replay. Returns the telemetry CSV and replay paths once the match UI disappears.
pub fn run_capture_loop(
    ui_map: &UiMap,
    templates: &Templates,
    record_frames: Option<PathBuf>,
) -> BoxResult<Option<(PathBuf, Option<PathBuf>)>> {
    let mut source = DxgiSource::new(Duration::from_millis(crate::constants::CAPTURE_INTERVAL_MS))?;
    let (w, h) = source.dimensions();
    let fps = 1000.0 / crate::constants::CAPTURE_INTERVAL_MS as f64;
    println!(
        "Started capture loop ({}x{}) at ~{:.1} FPS. Press Ctrl+C or close this window to exit.",
        w, h, fps
    );

    let opts = CaptureOptions {
        record_frames,
        ..Default::default()
    };
    let captured = match capture_match(&mut source, ui_map, templates, &opts)? {
        Some(captured) if captured.game_ended => captured,
        _ => return Ok(None),
    };

    println!("Capture loop finished successfully.");
    let (start, telemetry) = (captured.session_start, captured.telemetry);
    println!("[Discovery] Waiting 3s for game to write replay file...");
    std::thread::sleep(Duration::from_secs(3));
    if let Some(replay) = crate::replay_discovery::find_latest_replay(start, None) {
        println!("[Discovery] Discovered latest replay: {:?}", replay);
        Ok(Some((telemetry, Some(replay))))
    } else {
        eprintln!("[Discovery] Could not find a replay file modified after the session start.");
        if let Some(replay) = crate::replay_discovery::pick_replay_manually() {
            println!("[Discovery] Manually selected replay: {:?}", replay);
            Ok(Some((telemetry, Some(replay))))
        } else {
            println!("[Discovery] No replay selected. Fallback mode: generating report from telemetry data only.");
            Ok(Some((telemetry, None)))
        }
    }
}

/// Reads frames from `source` until a match has been recorded: waits for the match UI
/// (all resource fields readable), writes one telemetry row per frame (with housing
/// interpolated), and stops when the UI disappears or the source runs out.
/// Returns None if no match was seen.
pub fn capture_match<S: FrameSource>(
    source: &mut S,
    ui_map: &UiMap,
    templates: &Templates,
    opts: &CaptureOptions,
) -> BoxResult<Option<CapturedMatch>> {
    let mut state = CaptureState::WaitingForGame;
    let mut writer: Option<csv::Writer<std::fs::File>> = None;
    let mut engine = pipeline::interpolation::InterpolationEngine::new();
    let mut frame_idx = 0;
    let mut tick_count = 0;
    let mut session_start: Option<SystemTime> = None;
    let mut telemetry_path: Option<PathBuf> = None;
    let mut recorder: Option<FrameRecorder> = None;
    let mut frames_dir: Option<PathBuf> = None;

    loop {
        // Park the main thread indefinitely if we are shutting down via Ctrl-C
        if !crate::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::park();
        }

        let frame = match source.next_frame()? {
            SourceEvent::Frame(frame) => Some(frame),
            SourceEvent::Pending => None,
            SourceEvent::Exhausted => break,
        };

        if let Some(Frame {
            image,
            timestamp_ms,
        }) = frame
        {
            let start = Instant::now();
            let process_result = pipeline::process_frame(&image, ui_map, templates);

            match state {
                CaptureState::WaitingForGame => {
                    if crate::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
                        let spinner = ['|', '/', '-', '\\'];
                        print!(
                            "\rWaiting for match to start... {} ",
                            spinner[tick_count % 4]
                        );
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                        tick_count += 1;
                    }

                    if let Some(results) = process_result {
                        // Check if all major fields have a "total" value
                        let has_all_fields = ["food", "wood", "gold", "stone", "population"]
                            .iter()
                            .all(|&cat| {
                                results.get(cat).is_some_and(|m| {
                                    m.contains_key("total") && !m["total"].is_empty()
                                })
                            });

                        if has_all_fields {
                            if crate::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
                                println!(
                                    "\nMatch UI detected with all fields. Starting recording..."
                                );
                            }
                            state = CaptureState::Recording;
                            session_start = Some(UNIX_EPOCH + Duration::from_millis(timestamp_ms));

                            // Open writer
                            let stem = format!(
                                "telemetry_{}",
                                local_time(timestamp_ms).format("%Y%m%d_%H%M%S")
                            );
                            let _ = std::fs::create_dir_all(&opts.log_dir);
                            let path_buf = opts.log_dir.join(format!("{}.csv", stem));

                            let file = OpenOptions::new()
                                .write(true)
                                .create(true)
                                .truncate(true)
                                .open(&path_buf)?;

                            telemetry_path = Some(path_buf);
                            let mut w = csv::Writer::from_writer(file);
                            w.write_record(TELEMETRY_HEADER)?;
                            writer = Some(w);

                            if let Some(dir) = &opts.record_frames {
                                let dir = dir.join(&stem);
                                match FrameRecorder::start(&dir, opts.record.clone()) {
                                    Ok(r) => {
                                        println!("[Record] Saving frames to {}", dir.display());
                                        recorder = Some(r);
                                        frames_dir = Some(dir);
                                    }
                                    Err(e) => eprintln!(
                                        "[Record] Could not record frames in {}: {}",
                                        dir.display(),
                                        e
                                    ),
                                }
                            }
                        }
                    }
                }
                CaptureState::Recording => {
                    if let Some(results) = process_result {
                        if crate::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
                            let duration = start.elapsed();
                            print_telemetry(&results, duration);
                        }

                        // Prepare CapturedFrame for interpolation
                        let pop_curr = results
                            .get("population")
                            .and_then(|m| m.get("total"))
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);

                        let pop_max = results
                            .get("population")
                            .and_then(|m| m.get("housing"))
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);

                        let pop_color = results
                            .get("population")
                            .and_then(|m| m.get("color"))
                            .cloned()
                            .unwrap_or_else(|| "white".to_string());

                        let values = recorder
                            .as_ref()
                            .map(|_| PanelValues::from_results(&results));

                        let frame = pipeline::interpolation::CapturedFrame {
                            timestamp_ms,
                            frame_idx,
                            pop_color,
                            pop_curr,
                            pop_max,
                            row_data: results,
                        };

                        let interpolated_rows = engine.process_frame(frame);

                        if let Some(ref mut w) = writer {
                            write_rows(w, interpolated_rows)?;
                        }
                        if let (Some(r), Some(values)) = (recorder.as_mut(), values) {
                            r.offer(image, timestamp_ms, values);
                        }
                        frame_idx += 1;
                    } else {
                        if crate::RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
                            println!("Match UI lost. Ending recording...");
                        }
                        if let Some(r) = recorder.as_mut() {
                            r.offer_end(image, timestamp_ms);
                        }
                        state = CaptureState::GameEnded;
                        break;
                    }
                }
                CaptureState::GameEnded => break,
            }
        }

        source.end_tick();
    }

    // Write the rows still held back for housing interpolation (up to the 2 s queued
    // timeout before the match ended or the source ran out).
    if let Some(ref mut w) = writer {
        write_rows(w, engine.flush())?;
    }
    if let (Some(r), Some(dir)) = (recorder.take(), &frames_dir) {
        let summary = r.finish();
        print!(
            "[Record] Saved {} frames ({:.1} MB) to {}",
            summary.saved,
            summary.bytes as f64 / 1e6,
            dir.display()
        );
        if summary.dropped + summary.failed > 0 {
            print!(
                " ({} dropped while the writer was busy, {} failed)",
                summary.dropped, summary.failed
            );
        }
        println!();
    }

    Ok(session_start
        .zip(telemetry_path)
        .map(|(session_start, telemetry)| CapturedMatch {
            telemetry,
            session_start,
            game_ended: state == CaptureState::GameEnded,
            frames_dir,
        }))
}

fn write_rows(w: &mut csv::Writer<std::fs::File>, rows: Vec<(Results, u64)>) -> csv::Result<()> {
    for (row_data, row_timestamp_ms) in rows {
        w.write_record(prepare_telemetry_row(&row_data, row_timestamp_ms as u128))?;
    }
    let _ = w.flush();
    Ok(())
}

fn local_time(timestamp_ms: u64) -> chrono::DateTime<Local> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms as i64)
        .unwrap_or_default()
        .with_timezone(&Local)
}

fn print_telemetry(results: &crate::types::Results, duration: Duration) {
    let mut parts = Vec::new();
    let categories = ["food", "wood", "gold", "stone", "population", "idle"];

    for cat in categories {
        if let Some(data) = results.get(cat) {
            let total = data.get("total").map(|s| s.as_str()).unwrap_or("");
            let vils = data.get("vils").map(|s| s.as_str()).unwrap_or("");

            let label = match cat {
                "food" => "Food",
                "wood" => "Wood",
                "gold" => "Gold",
                "stone" => "Stone",
                "population" => "Pop",
                "idle" => "Idle",
                _ => cat,
            };

            if cat == "idle" {
                parts.push(format!("{}: {}", label, vils));
            } else if cat == "population" {
                let housing = data.get("housing").map(|s| s.as_str()).unwrap_or("");
                let pop_str = if !housing.is_empty() {
                    format!("{}/{}", total, housing)
                } else {
                    total.to_string()
                };
                if !vils.is_empty() {
                    parts.push(format!("{}: {} ({})", label, pop_str, vils));
                } else {
                    parts.push(format!("{}: {}", label, pop_str));
                }
            } else if !total.is_empty() && !vils.is_empty() {
                parts.push(format!("{}: {} ({})", label, total, vils));
            } else if !total.is_empty() {
                parts.push(format!("{}: {}", label, total));
            } else if !vils.is_empty() {
                parts.push(format!("{}: {}", label, vils));
            }
        }
    }
    let ui_scale = results
        .get("meta")
        .and_then(|m| m.get("ui_scale"))
        .map(|s| s.as_str())
        .unwrap_or("?");

    println!(
        "[Telemetry] Scale: {} | {} | Time: {}ms",
        ui_scale,
        parts.join(" | "),
        duration.as_millis()
    );
}

/// Converts a raw BGRA memory buffer (such as one mapped from DXGI) into an RgbImage.
///
/// `pitch` is the row pitch (bytes per row), which might be larger than `width * 4` due to padding.
pub fn bgra_to_rgb_image(data: &[u8], width: u32, height: u32, pitch: usize) -> RgbImage {
    let mut rgb_image = RgbImage::new(width, height);
    for y in 0..height {
        let row_start = y as usize * pitch;
        for x in 0..width {
            let px_start = row_start + (x as usize) * 4;
            // BGRA layout
            let b = data[px_start];
            let g = data[px_start + 1];
            let r = data[px_start + 2];
            // alpha is at px_start + 3, we ignore it

            rgb_image.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    rgb_image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgra_to_rgb_conversion() {
        // Create a 2x2 image with a padded pitch
        // Normal pitch would be 2 * 4 = 8 bytes. Let's make pitch 12 bytes to simulate alignment padding.
        let width = 2;
        let height = 2;
        let pitch = 12;

        let mut buffer = vec![0u8; height as usize * pitch];

        // Row 0, Col 0: Blue (B=255, G=0, R=0, A=255)
        buffer[0] = 255;
        buffer[1] = 0;
        buffer[2] = 0;
        buffer[3] = 255;
        // Row 0, Col 1: Green (B=0, G=255, R=0, A=255)
        buffer[4] = 0;
        buffer[5] = 255;
        buffer[6] = 0;
        buffer[7] = 255;
        // Padding bytes (8, 9, 10, 11) remain 0

        // Row 1, Col 0: Red (B=0, G=0, R=255, A=255)
        buffer[12] = 0;
        buffer[13] = 0;
        buffer[14] = 255;
        buffer[15] = 255;
        // Row 1, Col 1: White (B=255, G=255, R=255, A=255)
        buffer[16] = 255;
        buffer[17] = 255;
        buffer[18] = 255;
        buffer[19] = 255;

        let rgb_image = bgra_to_rgb_image(&buffer, width, height, pitch);

        assert_eq!(rgb_image.width(), 2);
        assert_eq!(rgb_image.height(), 2);

        // Check Row 0
        assert_eq!(rgb_image.get_pixel(0, 0).0, [0, 0, 255]); // Blue pixel -> RGB
        assert_eq!(rgb_image.get_pixel(1, 0).0, [0, 255, 0]); // Green pixel -> RGB

        // Check Row 1
        assert_eq!(rgb_image.get_pixel(0, 1).0, [255, 0, 0]); // Red pixel -> RGB
        assert_eq!(rgb_image.get_pixel(1, 1).0, [255, 255, 255]); // White pixel -> RGB
    }

    #[test]
    fn test_print_telemetry_logic() {
        // Since print_telemetry prints to stdout, we won't assert the output string directly
        // in this environment easily without capturing stdout, but we can verify it doesn't panic
        // and handles various result shapes.

        use std::collections::HashMap;
        let mut results = HashMap::new();

        let mut food = HashMap::new();
        food.insert("total".to_string(), "181".to_string());
        food.insert("vils".to_string(), "35".to_string());
        results.insert("food".to_string(), food);

        let mut idle = HashMap::new();
        idle.insert("vils".to_string(), "1".to_string());
        results.insert("idle".to_string(), idle);

        print_telemetry(&results, Duration::from_millis(42));
    }

    #[test]
    fn test_prepare_telemetry_row() {
        use std::collections::HashMap;
        let mut results = HashMap::new();

        let mut food = HashMap::new();
        food.insert("total".to_string(), "181".to_string());
        food.insert("vils".to_string(), "35".to_string());
        results.insert("food".to_string(), food);

        let mut pop = HashMap::new();
        pop.insert("total".to_string(), "15".to_string());
        pop.insert("housing".to_string(), "20".to_string());
        pop.insert("vils".to_string(), "12".to_string());
        results.insert("population".to_string(), pop);

        let mut idle = HashMap::new();
        idle.insert("vils".to_string(), "1".to_string());
        results.insert("idle".to_string(), idle);

        let timestamp = 123456789;
        let row = prepare_telemetry_row(&results, timestamp);

        assert_eq!(row[0], "123456789"); // timestamp
        assert_eq!(row[1], "181"); // food_total
        assert_eq!(row[2], "35"); // food_vils
        assert_eq!(row[9], "15"); // pop_curr
        assert_eq!(row[10], "20"); // pop_max
        assert_eq!(row[11], "12"); // pop_vils
        assert_eq!(row[12], "1"); // idle_vils
    }

    #[test]
    fn test_prepare_telemetry_row_partial() {
        use std::collections::HashMap;
        let mut results = HashMap::new();

        // Only population total, no slash
        let mut pop = HashMap::new();
        pop.insert("total".to_string(), "5".to_string());
        pop.insert("housing".to_string(), "".to_string());
        results.insert("population".to_string(), pop);

        let row = prepare_telemetry_row(&results, 0);

        assert_eq!(row[9], "5"); // pop_curr
        assert_eq!(row[10], ""); // pop_max
        assert_eq!(row[1], ""); // food_total (missing)
    }
}
