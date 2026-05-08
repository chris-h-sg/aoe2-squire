use crate::types::{Results, Templates, UiMap};
use chrono::Local;
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

#[derive(PartialEq)]
enum CaptureState {
    WaitingForGame,
    Recording,
    GameEnded,
}

pub fn run_capture_loop(
    ui_map: &UiMap,
    templates: &Templates,
) -> Result<Option<(std::path::PathBuf, std::path::PathBuf)>> {
    let mut session = CaptureSession::new()?;
    let (w, h) = session.dimensions();
    let fps = 1000.0 / crate::constants::CAPTURE_INTERVAL_MS as f64;
    println!("Started capture loop ({}x{}) at ~{:.1} FPS", w, h, fps);

    let mut state = CaptureState::WaitingForGame;
    let mut writer: Option<csv::Writer<std::fs::File>> = None;
    let mut engine = pipeline::interpolation::InterpolationEngine::new();
    let mut frame_idx = 0;
    let mut tick_count = 0;
    let mut session_start: Option<SystemTime> = None;
    let mut telemetry_path: Option<std::path::PathBuf> = None;
    let capture_interval = Duration::from_millis(crate::constants::CAPTURE_INTERVAL_MS);

    loop {
        let next_tick = std::time::Instant::now() + capture_interval;

        if let Some(dyn_image) = session.capture_frame()? {
            let start = std::time::Instant::now();
            let process_result = pipeline::process_frame(&dyn_image, ui_map, templates);

            match state {
                CaptureState::WaitingForGame => {
                    let spinner = ['|', '/', '-', '\\'];
                    print!(
                        "\rWaiting for game to start... {} ",
                        spinner[tick_count % 4]
                    );
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    tick_count += 1;

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
                            println!("\nGame UI detected with all fields. Starting recording...");
                            state = CaptureState::Recording;
                            session_start = Some(SystemTime::now());

                            // Open writer
                            let timestamp_str = Local::now().format("%Y%m%d_%H%M%S").to_string();
                            let log_path = format!("logs/telemetry_{}.csv", timestamp_str);
                            let _ = std::fs::create_dir_all("logs");
                            let path_buf = std::path::PathBuf::from(&log_path);

                            let file = OpenOptions::new()
                                .write(true)
                                .create(true)
                                .truncate(true)
                                .open(&path_buf)
                                .map_err(|e| {
                                    windows::core::Error::new(
                                        windows::core::HRESULT(-1),
                                        e.to_string(),
                                    )
                                })?;

                            telemetry_path = Some(path_buf);
                            let mut w = csv::Writer::from_writer(file);
                            w.write_record([
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
                            ])
                            .map_err(|e| {
                                windows::core::Error::new(windows::core::HRESULT(-1), e.to_string())
                            })?;

                            writer = Some(w);
                        }
                    }
                }
                CaptureState::Recording => {
                    if let Some(results) = process_result {
                        let duration = start.elapsed();

                        // Prepare CapturedFrame for interpolation
                        let timestamp_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis();

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

                        let frame = pipeline::interpolation::CapturedFrame {
                            timestamp_ms: timestamp_ms as u64,
                            frame_idx,
                            pop_color,
                            pop_curr,
                            pop_max,
                            row_data: results,
                        };

                        let interpolated_rows = engine.process_frame(frame);

                        if let Some(ref mut w) = writer {
                            for (row_data, row_timestamp_ms) in interpolated_rows {
                                print_telemetry(&row_data, duration);

                                let row =
                                    prepare_telemetry_row(&row_data, row_timestamp_ms as u128);
                                w.write_record(&row).map_err(|e| {
                                    windows::core::Error::new(
                                        windows::core::HRESULT(-1),
                                        e.to_string(),
                                    )
                                })?;
                            }
                            let _ = w.flush();
                        }
                        frame_idx += 1;
                    } else {
                        println!("Game UI lost. Ending recording...");
                        state = CaptureState::GameEnded;
                        break;
                    }
                }
                CaptureState::GameEnded => break,
            }
        }

        let now = std::time::Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        }
    }

    if state == CaptureState::GameEnded {
        println!("Capture loop finished successfully.");
        if let (Some(start), Some(telemetry)) = (session_start, telemetry_path) {
            println!("[Discovery] Waiting 3s for game to write replay file...");
            std::thread::sleep(Duration::from_secs(3));
            if let Some(replay) = crate::replay_discovery::find_latest_replay(start, None) {
                println!("[Discovery] Discovered latest replay: {:?}", replay);
                return Ok(Some((telemetry, replay)));
            } else {
                eprintln!(
                    "[Discovery] Could not find a replay file modified after the session start."
                );
                if let Some(replay) = crate::replay_discovery::pick_replay_manually() {
                    println!("[Discovery] Manually selected replay: {:?}", replay);
                    return Ok(Some((telemetry, replay)));
                }
            }
        }
    }

    Ok(None)
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
