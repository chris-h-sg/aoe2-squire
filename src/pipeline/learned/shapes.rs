//! Learned glyph shapes: per font set and label type, one high-resolution coverage image per
//! character, as the game draws it on screen (learned from screenshots by `learn_shapes`).
//!
//! File format (`data/digit_shapes/`): `<set>.json` holds the metadata and `<set>.png` (8-bit
//! grey) the coverage images, 0-255 for 0-1. Each image's cells are one font-atlas unit (the
//! font's 64 px em) square, so a glyph at font size `px` is drawn at `k = px / 64` screen pixels
//! per cell. In the JSON:
//! - `layout`: the panel layout the set belongs to (`layout.rs`);
//! - `pen_y`: per field, the pen height below the label box top, in atlas units;
//! - `kinds.count` / `kinds.total`: per character, its rectangle in the PNG (`x`, `y`, `w`, `h`
//!   in cells), the image origin relative to the pen (`ox`, `oy`, atlas units) and the pen
//!   advance to the next character (`adv`, atlas units, measured on the screenshots).

use super::layout::{Field, Kind, Layout};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Characters the reader knows.
pub const CHARS: &str = "0123456789/";

#[derive(Debug, Clone)]
pub struct Glyph {
    pub ch: char,
    /// Image origin relative to the pen, atlas units.
    pub ox: f32,
    pub oy: f32,
    /// Pen advance to the next character, atlas units (learned).
    pub adv: f32,
    pub gw: usize,
    pub gh: usize,
    /// Coverage 0-1, row-major `gw x gh`.
    pub cov: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct GlyphSet {
    pub glyphs: Vec<Glyph>,
}

impl GlyphSet {
    pub fn get(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.iter().find(|g| g.ch == ch)
    }
}

#[derive(Debug, Clone)]
pub struct ShapeSet {
    pub id: String,
    pub layout: &'static Layout,
    pub count: GlyphSet,
    pub total: GlyphSet,
    pub pen_y: HashMap<Field, f32>,
}

impl ShapeSet {
    pub fn glyphs(&self, kind: Kind) -> &GlyphSet {
        match kind {
            Kind::Count => &self.count,
            Kind::Total => &self.total,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct GlyphMeta {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub ox: f32,
    pub oy: f32,
    pub adv: f32,
}

#[derive(Serialize, Deserialize)]
pub struct SetMeta {
    pub id: String,
    pub layout: String,
    pub description: String,
    pub pen_y: BTreeMap<String, f32>,
    pub kinds: BTreeMap<String, BTreeMap<char, GlyphMeta>>,
}

impl ShapeSet {
    pub fn from_files(json: &str, png: &[u8]) -> Result<ShapeSet, String> {
        let meta: SetMeta = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(png)
            .map_err(|e| e.to_string())?
            .to_luma8();
        let layout =
            Layout::by_name(&meta.layout).ok_or(format!("unknown layout {}", meta.layout))?;
        let kind = |name: &str| -> GlyphSet {
            let mut glyphs = vec![];
            for (&ch, g) in meta.kinds.get(name).into_iter().flatten() {
                let cov = (0..g.w * g.h)
                    .map(|i| {
                        img.get_pixel((g.x + i % g.w) as u32, (g.y + i / g.w) as u32)
                            .0[0] as f32
                            / 255.0
                    })
                    .collect();
                glyphs.push(Glyph {
                    ch,
                    ox: g.ox,
                    oy: g.oy,
                    adv: g.adv,
                    gw: g.w,
                    gh: g.h,
                    cov,
                });
            }
            GlyphSet { glyphs }
        };
        let pen_y = meta
            .pen_y
            .iter()
            .filter_map(|(name, y)| Field::from_name(name).map(|f| (f, *y)))
            .collect();
        Ok(ShapeSet {
            id: meta.id,
            layout,
            count: kind("count"),
            total: kind("total"),
            pen_y,
        })
    }

    /// Writes `<dir>/<id>.json` and `<dir>/<id>.png`.
    pub fn save(&self, dir: &std::path::Path, description: &str) -> std::io::Result<()> {
        let mut kinds = BTreeMap::new();
        let all: Vec<(&str, &GlyphSet)> = vec![("count", &self.count), ("total", &self.total)];
        let width: usize = all
            .iter()
            .map(|(_, s)| s.glyphs.iter().map(|g| g.gw).sum::<usize>())
            .max()
            .unwrap_or(1);
        let height: usize = all
            .iter()
            .map(|(_, s)| s.glyphs.iter().map(|g| g.gh).max().unwrap_or(0))
            .sum::<usize>()
            .max(1);
        let mut img = image::GrayImage::new(width as u32, height as u32);
        let mut y = 0;
        for (name, set) in all {
            let mut x = 0;
            let mut row = BTreeMap::new();
            for g in &set.glyphs {
                for j in 0..g.gh {
                    for i in 0..g.gw {
                        let v = (g.cov[j * g.gw + i].clamp(0.0, 1.0) * 255.0).round() as u8;
                        img.put_pixel((x + i) as u32, (y + j) as u32, image::Luma([v]));
                    }
                }
                row.insert(
                    g.ch,
                    GlyphMeta {
                        x,
                        y,
                        w: g.gw,
                        h: g.gh,
                        ox: g.ox,
                        oy: g.oy,
                        adv: g.adv,
                    },
                );
                x += g.gw;
            }
            y += set.glyphs.iter().map(|g| g.gh).max().unwrap_or(0);
            kinds.insert(name.to_string(), row);
        }
        let meta = SetMeta {
            id: self.id.clone(),
            layout: self.layout.name.to_string(),
            description: description.to_string(),
            pen_y: self
                .pen_y
                .iter()
                .map(|(f, y)| (f.name().to_string(), (*y * 100.0).round() / 100.0))
                .collect(),
            kinds,
        };
        std::fs::create_dir_all(dir)?;
        std::fs::write(
            dir.join(format!("{}.json", self.id)),
            serde_json::to_string_pretty(&meta).unwrap() + "\n",
        )?;
        img.save(dir.join(format!("{}.png", self.id)))
            .map_err(std::io::Error::other)
    }
}

macro_rules! embedded {
    ($($id:literal),*) => {
        &[$(($id, include_str!(concat!("../../../data/digit_shapes/", $id, ".json")),
            include_bytes!(concat!("../../../data/digit_shapes/", $id, ".png")) as &[u8])),*]
    };
}

/// The shape sets shipped with the binary.
pub fn embedded_sets() -> Vec<ShapeSet> {
    let files: &[(&str, &str, &[u8])] = embedded!(
        "base_default",
        "base_sans",
        "anne_hk_default",
        "anne_hk_smooth",
        "anne_hk_sans"
    );
    files
        .iter()
        .map(|(id, json, png)| {
            ShapeSet::from_files(json, png).unwrap_or_else(|e| panic!("shape set {}: {}", id, e))
        })
        .collect()
}

fn overlap(a: f32, b: f32, c: f32, d: f32) -> f32 {
    (b.min(d) - a.max(c)).max(0.0)
}

/// Area-sampled footprint of glyph `g` with its pen at (px, py) (pixels of a `w x h` image) at
/// `k` pixels per atlas unit: calls `f(pixel index, cell index, overlap area)`.
pub fn stamp(
    g: &Glyph,
    k: f32,
    px: f32,
    py: f32,
    w: usize,
    h: usize,
    mut f: impl FnMut(usize, usize, f32),
) {
    let (ox, oy) = (px + g.ox * k, py + g.oy * k);
    let x0 = (ox.floor() as i32).max(0);
    let x1 = ((ox + g.gw as f32 * k).ceil() as i32).min(w as i32);
    let y0 = (oy.floor() as i32).max(0);
    let y1 = ((oy + g.gh as f32 * k).ceil() as i32).min(h as i32);
    for y in y0..y1 {
        let j0 = (((y as f32 - oy) / k).floor() as i32).max(0);
        let j1 = (((y as f32 + 1.0 - oy) / k).ceil() as i32).min(g.gh as i32);
        for x in x0..x1 {
            let i0 = (((x as f32 - ox) / k).floor() as i32).max(0);
            let i1 = (((x as f32 + 1.0 - ox) / k).ceil() as i32).min(g.gw as i32);
            for j in j0..j1 {
                let wy = overlap(
                    y as f32,
                    y as f32 + 1.0,
                    oy + j as f32 * k,
                    oy + (j + 1) as f32 * k,
                );
                if wy <= 0.0 {
                    continue;
                }
                for i in i0..i1 {
                    let wx = overlap(
                        x as f32,
                        x as f32 + 1.0,
                        ox + i as f32 * k,
                        ox + (i + 1) as f32 * k,
                    );
                    if wx > 0.0 {
                        f(
                            y as usize * w + x as usize,
                            j as usize * g.gw + i as usize,
                            wx * wy,
                        );
                    }
                }
            }
        }
    }
}

/// Coverage of a string of glyphs at the given pens (summed, capped at 1).
pub fn render(glyphs: &[(&Glyph, f32, f32)], k: f32, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    for &(g, x, y) in glyphs {
        stamp(g, k, x, y, w, h, |p, c, a| out[p] += a * g.cov[c]);
    }
    for v in out.iter_mut() {
        *v = v.min(1.0);
    }
    out
}

/// Total ink of a glyph in pixels at scale `k`.
pub fn ink(g: &Glyph, k: f32) -> f32 {
    g.cov.iter().sum::<f32>() * k * k
}
