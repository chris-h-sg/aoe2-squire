//! The learned-shape matcher: reads each number of the resource panel by finding the string of
//! glyphs whose rendering, from shapes learned off real screenshots, best explains the pixels.
//! No splitting into characters, sizes from the game's layout rule, a confidence per character.
//! See DECISION_LOG.md ("Learned Glyph Shapes").
//!
//! Font, layout (base or Anne_HK) and the shape set are detected on the first frames of a match
//! by reading them with every set and keeping the one that reads best.

pub mod layout;
pub mod observe;
pub mod read;
pub mod shapes;
pub mod train;

use super::{anchor, filter};
use crate::types::{Results, UiMap};
use image::{DynamicImage, RgbImage};
use layout::{px_per_unit, Field, Kind};
use observe::TextColour;
use read::{Grammar, Reading, Templates};
use shapes::ShapeSet;
use std::collections::HashMap;

/// A reading counts only if every character beats its best substitute, and the string its
/// runner-up, by this many grey levels (RMSE over the ink); otherwise the field is unread.
pub const MIN_MARGIN: f64 = 2.0;
/// Frames read with every shape set before keeping the best one.
const DETECT_FRAMES: usize = 3;

/// Panel strip copied out of each frame: everything any layout reads (layout units).
const STRIP_W: f32 = 1140.0;
const STRIP_H: f32 = 106.0;

/// One field's result.
#[derive(Debug, Clone)]
pub struct FieldReading {
    pub field: Field,
    /// None when no confident reading was found.
    pub reading: Option<Reading>,
    pub colour: TextColour,
}

impl FieldReading {
    pub fn text(&self) -> Option<&str> {
        self.reading
            .as_ref()
            .filter(|r| r.confidence() >= MIN_MARGIN)
            .map(|r| r.text.as_str())
    }
}

pub struct LearnedReader {
    sets: Vec<ShapeSet>,
    /// Index of the detected set, once detection is over.
    chosen: Option<usize>,
    /// Detection so far: per set, (confident fields, summed RMSE of read fields).
    votes: Vec<(usize, f64)>,
    frames_seen: usize,
    /// Templates per (set, kind, font px * 1000, rounded).
    templates: HashMap<(usize, Kind, i64), Templates>,
}

impl Default for LearnedReader {
    fn default() -> Self {
        Self::new()
    }
}

impl LearnedReader {
    pub fn new() -> Self {
        Self::with_sets(shapes::embedded_sets())
    }

    pub fn with_sets(sets: Vec<ShapeSet>) -> Self {
        let n = sets.len();
        Self {
            sets,
            chosen: None,
            votes: vec![(0, 0.0); n],
            frames_seen: 0,
            templates: HashMap::new(),
        }
    }

    /// Restricts the reader to one shape set (by id), skipping detection.
    pub fn force_set(&mut self, id: &str) -> bool {
        match self.sets.iter().position(|s| s.id == id) {
            Some(i) => {
                self.chosen = Some(i);
                true
            }
            None => false,
        }
    }

    pub fn set_id(&self) -> Option<&str> {
        self.chosen.map(|i| self.sets[i].id.as_str())
    }

    fn templates(&mut self, set: usize, kind: Kind, k: f32) -> &Templates {
        let key = (set, kind, (k * 1e5).round() as i64);
        let sets = &self.sets;
        self.templates
            .entry(key)
            .or_insert_with(|| Templates::build(sets[set].glyphs(kind), k))
    }

    /// Reads every field of `strip` (the panel strip, at frame origin) with shape set `si`.
    pub fn read_fields(&mut self, si: usize, strip: &RgbImage, ui_scale: f64) -> Vec<FieldReading> {
        let u = px_per_unit(ui_scale);
        let layout = self.sets[si].layout;
        let mut out = vec![];
        for fl in &layout.fields {
            let field = fl.field;
            let colour = if field == Field::PopTotal {
                observe::text_colour(strip, (0, 0), fl, u)
            } else {
                TextColour::White
            };
            let rgb = if field == Field::PopTotal {
                colour.rgb()
            } else {
                fl.colour
            };
            let Some(obs) = observe::observe(strip, (0, 0), fl, u, rgb, None) else {
                out.push(FieldReading {
                    field,
                    reading: None,
                    colour,
                });
                continue;
            };
            let pen_y = self.sets[si].pen_y.get(&field).copied().unwrap_or(0.0);
            let grammar = if field == Field::PopTotal {
                Grammar::Ratio
            } else {
                Grammar::Number
            };
            let k = obs.k;
            let t = self.templates(si, field.kind(), k);
            let reading = read::read(&obs, t, grammar, field.max_len(), obs.top + pen_y * k);
            out.push(FieldReading {
                field,
                reading,
                colour,
            });
        }
        out
    }

    /// Reads one frame. None if the game's panel is not found (same rule as the split matcher).
    pub fn process_frame(&mut self, img: &DynamicImage, ui_map: &UiMap) -> Option<Results> {
        let ui_scale = anchor::detect_ui_scale(img, ui_map.baseline_margin)?;
        let u = px_per_unit(ui_scale);
        let (sw, sh) = (
            ((STRIP_W * u).ceil() as u32).min(img.width()),
            ((STRIP_H * u).ceil() as u32).min(img.height()),
        );
        let strip = img.crop_imm(0, 0, sw, sh).to_rgb8();

        let si = match self.chosen {
            Some(i) => i,
            None => {
                let mut best = 0;
                for si in 0..self.sets.len() {
                    let reads = self.read_fields(si, &strip, ui_scale);
                    let read: Vec<&Reading> = reads
                        .iter()
                        .filter(|r| r.text().is_some())
                        .filter_map(|r| r.reading.as_ref())
                        .collect();
                    self.votes[si].0 += read.len();
                    self.votes[si].1 += read.iter().map(|r| r.rmse).sum::<f64>();
                    if (self.votes[si].0, -self.votes[si].1)
                        > (self.votes[best].0, -self.votes[best].1)
                    {
                        best = si;
                    }
                }
                self.frames_seen += 1;
                if self.frames_seen >= DETECT_FRAMES {
                    self.chosen = Some(best);
                }
                best
            }
        };
        let reads = self.read_fields(si, &strip, ui_scale);
        Some(self.results(si, &reads, &strip, ui_scale))
    }

    fn results(
        &self,
        si: usize,
        reads: &[FieldReading],
        strip: &RgbImage,
        ui_scale: f64,
    ) -> Results {
        let mut results = Results::new();
        let mut put = |cat: &str, key: &str, v: String| {
            results
                .entry(cat.to_string())
                .or_default()
                .insert(key.to_string(), v);
        };
        put("meta", "ui_scale", format!("{:.4}", ui_scale));
        put("meta", "shape_set", self.sets[si].id.clone());
        let u = px_per_unit(ui_scale);
        let layout = self.sets[si].layout;
        let mut pop_color = "white";
        for r in reads {
            let text = r.text().unwrap_or("").to_string();
            let (cat, key) = r.field.name().split_once('_').unwrap();
            match r.field {
                Field::PopTotal => {
                    let (curr, max) = text.split_once('/').unwrap_or(("", ""));
                    put("population", "total", curr.to_string());
                    put("population", "housing", max.to_string());
                    let fl = layout.field(Field::PopTotal);
                    let region = crop(strip, fl.crop, u);
                    pop_color = if region.as_ref().is_some_and(filter::detect_housed_overlay) {
                        "overlay"
                    } else {
                        match r.colour {
                            TextColour::White => "white",
                            TextColour::Yellow => "yellow",
                            TextColour::Red => "red",
                        }
                    };
                }
                Field::IdleVils => {
                    // No idle villagers: the icon is greyed out (base) or not red (Anne_HK).
                    let value = if text.is_empty() {
                        let fl = layout.field(Field::IdleVils);
                        let active = crop(strip, fl.crop, u).is_some_and(|b| {
                            if layout.name == "anne_hk" {
                                filter::contains_red(&b)
                            } else {
                                filter::contains_yellow(&b)
                            }
                        });
                        if active {
                            text
                        } else {
                            "0".to_string()
                        }
                    } else {
                        text
                    };
                    put("idle", "vils", value);
                }
                _ => put(cat, key, text),
            }
        }
        put("population", "color", pop_color.to_string());
        results
    }
}

fn crop(strip: &RgbImage, c: [f32; 4], u: f32) -> Option<RgbImage> {
    let (x0, y0) = ((c[0] * u).floor() as u32, (c[1] * u).floor() as u32);
    let (x1, y1) = (
        ((c[2] * u).ceil() as u32).min(strip.width()),
        ((c[3] * u).ceil() as u32).min(strip.height()),
    );
    (x1 > x0 && y1 > y0)
        .then(|| image::imageops::crop_imm(strip, x0, y0, x1 - x0, y1 - y0).to_image())
}
