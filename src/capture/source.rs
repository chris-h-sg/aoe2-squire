//! Frame sources for the capture loop: live DXGI capture (`super::DxgiSource`) or a
//! recorded sequence of images (`ImageSequenceSource`), so the loop can run without a game.

use image::DynamicImage;
use serde::Deserialize;
use std::error::Error;
use std::path::{Path, PathBuf};

/// Sidecar index written by the frame recorder: `timestamp_ms,file,reason`.
pub const FRAMES_INDEX: &str = "frames.csv";

/// A captured screen. `image` is always RGB8 (as delivered by DXGI); `timestamp_ms` is
/// milliseconds since the Unix epoch at capture time and becomes the telemetry row timestamp.
pub struct Frame {
    pub image: DynamicImage,
    pub timestamp_ms: u64,
}

pub enum SourceEvent {
    Frame(Frame),
    /// No new frame this tick (e.g. the screen did not change); try again next tick.
    Pending,
    /// The source has no more frames; the capture loop stops.
    Exhausted,
}

pub trait FrameSource {
    fn next_frame(&mut self) -> Result<SourceEvent, Box<dyn Error>>;

    /// Called once per loop iteration after the frame has been processed. Live sources
    /// sleep here to hold the capture rate; recorded sources return immediately.
    fn end_tick(&mut self) {}
}

/// Replays still images in timestamp order, e.g. frames saved with `--record-frames`.
pub struct ImageSequenceSource {
    frames: std::vec::IntoIter<(PathBuf, u64)>,
}

#[derive(Deserialize)]
struct IndexRow {
    timestamp_ms: u64,
    file: String,
}

impl ImageSequenceSource {
    /// `frames` are (image path, timestamp_ms) pairs; they are replayed sorted by timestamp.
    pub fn new(mut frames: Vec<(PathBuf, u64)>) -> Self {
        frames.sort_by_key(|(_, ts)| *ts);
        Self {
            frames: frames.into_iter(),
        }
    }

    /// Loads a directory of frames. Timestamps come from `frames.csv` (columns
    /// `timestamp_ms` and `file`, relative to `dir`) if present; otherwise every
    /// `<timestamp_ms>.png` in the directory is used.
    pub fn from_dir(dir: &Path) -> Result<Self, Box<dyn Error>> {
        let index = dir.join(FRAMES_INDEX);
        let mut frames = vec![];
        if index.exists() {
            for row in csv::Reader::from_path(&index)?.deserialize() {
                let row: IndexRow = row?;
                frames.push((dir.join(row.file), row.timestamp_ms));
            }
        } else {
            for entry in std::fs::read_dir(dir)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                {
                    if let Some(ts) = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.parse().ok())
                    {
                        frames.push((path, ts));
                    }
                }
            }
        }
        if frames.is_empty() {
            return Err(format!("no frames found in {}", dir.display()).into());
        }
        Ok(Self::new(frames))
    }

    pub fn remaining(&self) -> usize {
        self.frames.len()
    }
}

impl FrameSource for ImageSequenceSource {
    fn next_frame(&mut self) -> Result<SourceEvent, Box<dyn Error>> {
        let Some((path, timestamp_ms)) = self.frames.next() else {
            return Ok(SourceEvent::Exhausted);
        };
        let image = image::open(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Ok(SourceEvent::Frame(Frame {
            image: DynamicImage::ImageRgb8(image.into_rgb8()),
            timestamp_ms,
        }))
    }
}
