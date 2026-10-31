//! Opt-in frame recording (`--record-frames <dir>`): saves masked PNG frames of a match so
//! the readings can be checked frame by frame afterwards and turned into vision fixtures.
//!
//! The capture thread only decides which frames to keep and hands the (already captured)
//! image over a bounded channel; masking, PNG encoding and disk writes run on a
//! low-priority worker thread. When the worker falls behind, frames are dropped rather
//! than slowing down the capture loop.

use super::source::FRAMES_INDEX;
use crate::types::Results;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, RgbImage};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::Instant;

/// Regions kept by `mask_panel` as (x range, y range) fractions of the frame; everything
/// else is blacked out. Must match KEEP_REGIONS in tools/extract_video_fixtures.py, which
/// explains how they were chosen (resource panel, and the part of the anchor search band
/// where the anchor can be).
pub const KEEP_REGIONS: [((f64, f64), (f64, f64)); 2] =
    [((0.0, 0.4), (0.0, 0.07)), ((0.75, 1.0), (0.0, 0.05))];

/// Returns a full-size copy of `img` with everything outside `KEEP_REGIONS` black. The
/// frame keeps its size because the anchor search uses fractions of the frame size.
pub fn mask_panel(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    let mut out = vec![0u8; img.as_raw().len()];
    let stride = w as usize * 3;
    let src = img.as_raw();
    for ((x0, x1), (y0, y1)) in KEEP_REGIONS {
        let (xa, xb) = ((x0 * w as f64) as usize, (x1 * w as f64) as usize);
        let (ya, yb) = ((y0 * h as f64) as usize, (y1 * h as f64) as usize);
        for y in ya..yb {
            let row = y * stride + xa * 3..y * stride + xb * 3;
            out[row.clone()].copy_from_slice(&src[row]);
        }
    }
    RgbImage::from_raw(w, h, out).unwrap()
}

/// Encodes `img` as a PNG file.
pub fn save_png(img: &RgbImage, path: &Path) -> image::ImageResult<()> {
    let file = BufWriter::new(File::create(path)?);
    PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Adaptive).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgb8,
    )
}

const TOTAL_FIELDS: [(&str, &str, &str); 4] = [
    ("food", "total", "food_total"),
    ("wood", "total", "wood_total"),
    ("gold", "total", "gold_total"),
    ("stone", "total", "stone_total"),
];

const COUNT_FIELDS: [(&str, &str, &str); 8] = [
    ("food", "vils", "food_vils"),
    ("wood", "vils", "wood_vils"),
    ("gold", "vils", "gold_vils"),
    ("stone", "vils", "stone_vils"),
    ("population", "total", "pop_curr"),
    ("population", "housing", "pop_max"),
    ("population", "vils", "pop_vils"),
    ("idle", "vils", "idle_vils"),
];

/// The panel readings the recorder compares between consecutive frames.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelValues {
    totals: [Option<i64>; 4],
    counts: [Option<i64>; 8],
    color: Option<String>,
}

impl PanelValues {
    pub fn from_results(results: &Results) -> Self {
        let get = |cat: &str, sub: &str| results.get(cat).and_then(|m| m.get(sub));
        let num = |&(cat, sub, _): &(&str, &str, &str)| get(cat, sub).and_then(|s| s.parse().ok());
        Self {
            totals: TOTAL_FIELDS.map(|f| num(&f)),
            counts: COUNT_FIELDS.map(|f| num(&f)),
            color: get("population", "color").cloned(),
        }
    }

    /// Names the first field that changed sharply since `prev`: a resource total moved by
    /// at least `resource_jump`, a count by at least `count_jump`, a field appeared or
    /// disappeared, or the population colour changed (housing overlay / queue).
    fn sharp_change(&self, prev: &Self, opts: &RecordOptions) -> Option<&'static str> {
        let jumped = |a: Option<i64>, b: Option<i64>, threshold: i64| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() >= threshold,
            (a, b) => a.is_some() != b.is_some(),
        };
        for (i, f) in TOTAL_FIELDS.iter().enumerate() {
            if jumped(prev.totals[i], self.totals[i], opts.resource_jump) {
                return Some(f.2);
            }
        }
        for (i, f) in COUNT_FIELDS.iter().enumerate() {
            if jumped(prev.counts[i], self.counts[i], opts.count_jump) {
                return Some(f.2);
            }
        }
        (prev.color != self.color).then_some("pop_color")
    }
}

#[derive(Clone, Debug)]
pub struct RecordOptions {
    /// Save a frame at least this often.
    pub interval_ms: u64,
    /// Extra frames for sharp changes are at least this far apart.
    pub min_gap_ms: u64,
    pub resource_jump: i64,
    pub count_jump: i64,
    /// Frames waiting for the worker; further frames are dropped.
    pub queue_capacity: usize,
}

impl Default for RecordOptions {
    fn default() -> Self {
        use crate::constants::*;
        Self {
            interval_ms: RECORD_INTERVAL_MS,
            min_gap_ms: RECORD_MIN_GAP_MS,
            resource_jump: RECORD_RESOURCE_JUMP,
            count_jump: RECORD_COUNT_JUMP,
            queue_capacity: RECORD_QUEUE_CAPACITY,
        }
    }
}

struct Job {
    image: DynamicImage,
    timestamp_ms: u64,
    reason: String,
}

#[derive(Debug, Default)]
pub struct RecordingSummary {
    pub saved: usize,
    pub dropped: usize,
    pub failed: usize,
    pub bytes: u64,
    /// Worker time spent masking, encoding and writing, in ms.
    pub worker_ms: f64,
}

struct Previous {
    /// None once the frame has been handed to the worker.
    image: Option<DynamicImage>,
    timestamp_ms: u64,
    values: PanelValues,
}

/// Decides which frames of a match to save and sends them to the worker thread, which
/// writes `<timestamp_ms>.png` and a `frames.csv` line (`timestamp_ms,file,reason`) for
/// each. Every saved frame except the `end` frame has a telemetry row with the same
/// timestamp.
pub struct FrameRecorder {
    opts: RecordOptions,
    tx: Option<SyncSender<Job>>,
    worker: Option<JoinHandle<RecordingSummary>>,
    last_saved_ms: Option<u64>,
    prev: Option<Previous>,
    dropped: usize,
}

impl FrameRecorder {
    pub fn start(dir: &Path, opts: RecordOptions) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut index = BufWriter::new(File::create(dir.join(FRAMES_INDEX))?);
        writeln!(index, "timestamp_ms,file,reason")?;
        index.flush()?;
        let (tx, rx) = sync_channel(opts.queue_capacity);
        let dir = dir.to_path_buf();
        let worker = std::thread::Builder::new()
            .name("frame-recorder".into())
            .spawn(move || run_worker(dir, index, rx))?;
        Ok(Self {
            opts,
            tx: Some(tx),
            worker: Some(worker),
            last_saved_ms: None,
            prev: None,
            dropped: 0,
        })
    }

    /// Offers a frame that produced a telemetry row. Saves it if the save interval has
    /// passed or a reading changed sharply. For a jump in a number, the frame before is
    /// saved too (if it was not), to tell a misread from a real change; not for a colour
    /// change, as the housed overlay blinks every second.
    pub fn offer(&mut self, image: DynamicImage, timestamp_ms: u64, values: PanelValues) {
        let since_save = self.last_saved_ms.map(|t| timestamp_ms.saturating_sub(t));
        let change = match (&self.prev, since_save) {
            (Some(prev), Some(gap)) if gap >= self.opts.min_gap_ms => {
                values.sharp_change(&prev.values, &self.opts)
            }
            _ => None,
        };
        let reason = match (change, since_save) {
            (Some(field), _) => Some(format!("change:{}", field)),
            (None, None) => Some("first".to_string()),
            (None, Some(gap)) if gap >= self.opts.interval_ms => Some("interval".to_string()),
            _ => None,
        };

        let mut image = Some(image);
        if let Some(reason) = reason {
            if change.is_some_and(|field| field != "pop_color") {
                if let Some((prev_image, prev_ts)) = self
                    .prev
                    .as_mut()
                    .and_then(|p| Some((p.image.take()?, p.timestamp_ms)))
                {
                    self.send(prev_image, prev_ts, "before".to_string());
                }
            }
            self.send(image.take().unwrap(), timestamp_ms, reason);
            self.last_saved_ms = Some(timestamp_ms);
        }
        self.prev = Some(Previous {
            image,
            timestamp_ms,
            values,
        });
    }

    /// Saves the frame on which the match UI was lost (it has no telemetry row).
    pub fn offer_end(&mut self, image: DynamicImage, timestamp_ms: u64) {
        self.send(image, timestamp_ms, "end".to_string());
    }

    fn send(&mut self, image: DynamicImage, timestamp_ms: u64, reason: String) {
        let job = Job {
            image,
            timestamp_ms,
            reason,
        };
        // A full queue means the worker is behind: drop the frame instead of waiting.
        if !matches!(self.tx.as_ref().map(|tx| tx.try_send(job)), Some(Ok(()))) {
            self.dropped += 1;
        }
    }

    /// Waits for the worker to write the queued frames.
    pub fn finish(mut self) -> RecordingSummary {
        self.join()
    }

    fn join(&mut self) -> RecordingSummary {
        self.tx = None;
        let mut summary = self
            .worker
            .take()
            .and_then(|w| w.join().ok())
            .unwrap_or_default();
        summary.dropped += self.dropped;
        summary
    }
}

impl Drop for FrameRecorder {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.join();
        }
    }
}

fn run_worker(dir: PathBuf, mut index: BufWriter<File>, rx: Receiver<Job>) -> RecordingSummary {
    lower_thread_priority();
    let mut summary = RecordingSummary::default();
    for job in rx {
        let start = Instant::now();
        let file = format!("{}.png", job.timestamp_ms);
        let path = dir.join(&file);
        let masked = mask_panel(&job.image.into_rgb8());
        let result = save_png(&masked, &path)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                writeln!(index, "{},{},{}", job.timestamp_ms, file, job.reason)
                    .and_then(|_| index.flush())
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                summary.saved += 1;
                summary.bytes += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            }
            Err(e) => {
                if summary.failed == 0 {
                    eprintln!("[Record] Could not save {}: {}", path.display(), e);
                }
                summary.failed += 1;
            }
        }
        summary.worker_ms += start.elapsed().as_secs_f64() * 1000.0;
    }
    summary
}

/// Keeps PNG encoding from competing with the game for CPU time.
fn lower_thread_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn values(wood: &str, wood_vils: &str, color: &str) -> PanelValues {
        let mut r: Results = HashMap::new();
        let mut set = |cat: &str, sub: &str, v: &str| {
            r.entry(cat.to_string())
                .or_default()
                .insert(sub.to_string(), v.to_string());
        };
        set("wood", "total", wood);
        set("wood", "vils", wood_vils);
        set("population", "color", color);
        PanelValues::from_results(&r)
    }

    #[test]
    fn test_sharp_change() {
        let opts = RecordOptions::default();
        let base = values("500", "10", "white");
        assert_eq!(
            values("520", "11", "white").sharp_change(&base, &opts),
            None
        );
        assert_eq!(
            values("50", "10", "white").sharp_change(&base, &opts),
            Some("wood_total")
        );
        assert_eq!(
            values("", "10", "white").sharp_change(&base, &opts),
            Some("wood_total")
        );
        assert_eq!(
            values("500", "14", "white").sharp_change(&base, &opts),
            Some("wood_vils")
        );
        assert_eq!(
            values("500", "10", "overlay").sharp_change(&base, &opts),
            Some("pop_color")
        );
    }

    #[test]
    fn test_mask_panel_keeps_only_regions() {
        let img = RgbImage::from_pixel(200, 100, image::Rgb([9, 9, 9]));
        let masked = mask_panel(&img);
        assert_eq!(masked.dimensions(), (200, 100));
        assert_eq!(masked.get_pixel(0, 0).0, [9, 9, 9]);
        assert_eq!(masked.get_pixel(79, 6).0, [9, 9, 9]);
        assert_eq!(masked.get_pixel(80, 6).0, [0, 0, 0]); // x = 40%
        assert_eq!(masked.get_pixel(10, 7).0, [0, 0, 0]); // y = 7%
        assert_eq!(masked.get_pixel(150, 4).0, [9, 9, 9]);
        assert_eq!(masked.get_pixel(199, 5).0, [0, 0, 0]); // y = 5%
        assert_eq!(masked.get_pixel(149, 0).0, [0, 0, 0]); // x < 75%
        assert_eq!(masked.get_pixel(100, 50).0, [0, 0, 0]);
    }
}
