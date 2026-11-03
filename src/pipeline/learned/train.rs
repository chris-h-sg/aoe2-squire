//! Learning glyph shapes from screenshots with known values (used by `learn_shapes`).
//!
//! Every character's shape is a coverage image on a grid of one font-atlas unit, started from
//! the font atlas (MSDF inside/outside at 4x4 samples per cell). Training alternates:
//! - placing each glyph of each observation at its best sub-pixel pen (exhaustive search, then
//!   finer steps), with the shapes fixed;
//! - projected gradient descent on all shapes at once (squared error over every observation,
//!   plus a small smoothness term), with the positions fixed.
//!
//! Because the screenshots show each glyph at many sub-pixel positions (and, for the base
//! font, at many UI sizes), the shapes come out at a finer resolution than any one screenshot.

use super::layout::Field;
use super::observe::Obs;
use super::read::{cost_at, render_error};
use super::shapes::{render, stamp, Glyph, GlyphSet, CHARS};
use std::collections::HashMap;

/// Free border around each glyph's atlas quad, atlas units.
pub const PAD: f32 = 6.0;

/// One field of one screenshot with its known text.
pub struct Sample {
    pub field: Field,
    pub text: String,
    pub obs: Obs,
    /// Fitted pens (region pixels), one per character.
    pub pens: Vec<(f32, f32)>,
}

/// Font metrics of one glyph from the game's `<font>.txt`.
#[derive(Debug, Clone, Copy)]
pub struct FontGlyph {
    pub w: f32,
    pub h: f32,
    pub u: f32,
    pub v: f32,
    pub xo: f32,
    pub yo: f32,
    pub adv: f32,
    pub page: u32,
}

/// Reads the metrics of the digits and '/' from `<fonts>/<font>.txt`.
pub fn load_font(fonts: &std::path::Path, font: &str) -> Result<HashMap<char, FontGlyph>, String> {
    let txt = std::fs::read_to_string(fonts.join(format!("{font}.txt")))
        .map_err(|e| format!("{font}.txt: {e}"))?;
    let mut g = HashMap::new();
    for line in txt.lines() {
        let Some(rest) = line.strip_prefix("Glyph - '") else {
            continue;
        };
        let ch = rest.chars().next().unwrap();
        if !CHARS.contains(ch) {
            continue;
        }
        let num = |key: &str| -> Vec<f32> {
            let Some(i) = line.find(&format!("{key}(")) else {
                return vec![0.0; 2];
            };
            let s = &line[i..];
            let (a, b) = (s.find('(').unwrap(), s.find(')').unwrap());
            s[a + 1..b]
                .split(',')
                .map(|t| t.trim().parse().unwrap_or(0.0))
                .collect()
        };
        let uv = num("UV");
        g.insert(
            ch,
            FontGlyph {
                w: num("W")[0],
                h: num("H")[0],
                u: uv[0],
                v: uv[1],
                xo: num("XO")[0],
                yo: num("YO")[0],
                adv: num("HAdvance")[0],
                page: num("Atlas")[0] as u32,
            },
        );
    }
    Ok(g)
}

/// Starting shapes from the font atlas: the share of each cell's 4x4 samples inside the glyph
/// (median of the MSDF channels > 0.5).
pub fn atlas_shapes(
    fonts: &std::path::Path,
    font: &str,
    metrics: &HashMap<char, FontGlyph>,
) -> Result<GlyphSet, String> {
    let mut pages: HashMap<u32, image::RgbaImage> = HashMap::new();
    let mut glyphs = vec![];
    for ch in CHARS.chars() {
        let Some(g) = metrics.get(&ch) else { continue };
        if let std::collections::hash_map::Entry::Vacant(e) = pages.entry(g.page) {
            let path = fonts.join(format!("{font}_{:04}.png", g.page));
            e.insert(
                image::open(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .to_rgba8(),
            );
        }
        let page = &pages[&g.page];
        let (pw, phh) = (page.width() as f32, page.height() as f32);
        let (gw, gh) = (
            (g.w + 2.0 * PAD).ceil() as usize,
            (g.h + 2.0 * PAD).ceil() as usize,
        );
        let (au, av) = (g.u * pw, g.v * phh);
        let mut cov = vec![0f32; gw * gh];
        for j in 0..gh {
            for i in 0..gw {
                let mut c = 0.0;
                for a in 0..4 {
                    for b in 0..4 {
                        let x = au - PAD + i as f32 + (a as f32 + 0.5) / 4.0;
                        let y = av - PAD + j as f32 + (b as f32 + 0.5) / 4.0;
                        let p = page
                            .get_pixel(
                                (x.floor().max(0.0) as u32).min(page.width() - 1),
                                (y.floor().max(0.0) as u32).min(page.height() - 1),
                            )
                            .0;
                        let [r, gg, bb] = [p[0], p[1], p[2]];
                        let med = r.max(gg).min(bb).max(r.min(gg));
                        if med > 127 {
                            c += 1.0 / 16.0;
                        }
                    }
                }
                cov[j * gw + i] = c;
            }
        }
        glyphs.push(Glyph {
            ch,
            ox: g.xo - PAD,
            oy: g.yo - PAD,
            adv: g.adv,
            gw,
            gh,
            cov,
        });
    }
    Ok(GlyphSet { glyphs })
}

fn placed<'a>(set: &'a GlyphSet, text: &str, pens: &[(f32, f32)]) -> Vec<(&'a Glyph, f32, f32)> {
    text.chars()
        .zip(pens)
        .map(|(c, p)| (set.get(c).unwrap(), p.0, p.1))
        .collect()
}

/// Best pens for the sample's text: with `coarse`, an exhaustive half-pixel search of the
/// string as a whole first; then each glyph alone in finer steps.
pub fn fit_pens(s: &mut Sample, set: &GlyphSet, coarse: bool) {
    let o = &s.obs;
    let n = s.text.chars().count();
    let mut pens = if s.pens.len() == n {
        s.pens.clone()
    } else {
        vec![(0.0, 0.0); n]
    };
    let mut cur = cost_at(o, &placed(set, &s.text, &pens));
    if coarse {
        // the string as a whole: one uniform advance for every character (on screen the slash
        // advances about as far as a digit), from the font's digit advance -1 to +5 units (the
        // game rounds pens to pixels, so the gaps on screen are usually wider)
        let font_adv = set.get('0').map_or(26.5, |g| g.adv);
        for extra in -1..=5 {
            let adv = (font_adv + extra as f32) * o.k;
            for i in -4..(o.w as i32 * 2) {
                for j in -8..(o.h as i32 * 2) {
                    let p: Vec<(f32, f32)> = (0..n)
                        .map(|g| (i as f32 * 0.5 + g as f32 * adv, j as f32 * 0.5))
                        .collect();
                    let c = cost_at(o, &placed(set, &s.text, &p));
                    if c < cur {
                        cur = c;
                        pens = p;
                    }
                }
            }
        }
    }
    // then each glyph alone: +-4 px in half pixels (glyphs after a slash can be off by that
    // much), then finer
    let stages: &[(i32, f32, usize)] = if coarse {
        &[(8, 0.5, 2), (4, 0.25, 2), (4, 0.0625, 2), (2, 0.03125, 1)]
    } else {
        &[(4, 0.25, 2), (4, 0.0625, 2), (2, 0.03125, 1)]
    };
    for &(range, step, rounds) in stages {
        for _ in 0..rounds {
            for g in 0..n {
                let b = pens[g];
                for i in -range..=range {
                    for j in -range..=range {
                        let mut p = pens.clone();
                        p[g] = (b.0 + i as f32 * step, b.1 + j as f32 * step);
                        let c = cost_at(o, &placed(set, &s.text, &p));
                        if c < cur {
                            cur = c;
                            pens = p;
                        }
                    }
                }
            }
        }
    }
    s.pens = pens;
}

/// Projected gradient descent on all shapes of `set` over the samples (positions fixed):
/// squared error + `lambda` * squared differences between neighbouring cells.
pub fn fit_shapes(samples: &[Sample], set: &mut GlyphSet, lambda: f32, iters: usize) {
    let total = |set: &GlyphSet| -> f64 {
        samples
            .iter()
            .map(|s| {
                let o = &s.obs;
                render_error(o, &render(&placed(set, &s.text, &s.pens), o.k, o.w, o.h))
            })
            .sum()
    };
    let mut lr = 0.5f32;
    let mut last = total(set);
    for _ in 0..iters {
        let mut grad: Vec<Vec<f32>> = set.glyphs.iter().map(|g| vec![0f32; g.cov.len()]).collect();
        for s in samples {
            let o = &s.obs;
            let r = render(&placed(set, &s.text, &s.pens), o.k, o.w, o.h);
            for (c, p) in s.text.chars().zip(&s.pens) {
                let gi = set.glyphs.iter().position(|g| g.ch == c).unwrap();
                let gr = &mut grad[gi];
                stamp(&set.glyphs[gi], o.k, p.0, p.1, o.w, o.h, |px, cell, a| {
                    if r[px] < 1.0 {
                        let e = if o.mask[px] {
                            o.wt[px] * (r[px] - o.img[px])
                        } else {
                            r[px]
                        };
                        gr[cell] += 2.0 * e * a;
                    }
                });
            }
        }
        let old: Vec<Vec<f32>> = set.glyphs.iter().map(|g| g.cov.clone()).collect();
        for (g, gr) in set.glyphs.iter_mut().zip(&grad) {
            let (gw, gh) = (g.gw, g.gh);
            let prev = g.cov.clone();
            for j in 0..gh {
                for i in 0..gw {
                    let c = j * gw + i;
                    let mut sm = 0.0;
                    for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let (ii, jj) = (i as i32 + di, j as i32 + dj);
                        if ii >= 0 && jj >= 0 && (ii as usize) < gw && (jj as usize) < gh {
                            sm += prev[c] - prev[jj as usize * gw + ii as usize];
                        }
                    }
                    g.cov[c] = (prev[c] - lr * (gr[c] + 2.0 * lambda * sm)).clamp(0.0, 1.0);
                }
            }
        }
        let now = total(set);
        if now > last {
            for (g, o) in set.glyphs.iter_mut().zip(old) {
                g.cov = o;
            }
            lr *= 0.5;
        } else {
            last = now;
            lr *= 1.1;
        }
    }
}

/// Learns one label type's shapes from its samples, starting from `init`.
pub fn learn_kind(
    samples: &mut [Sample],
    init: GlyphSet,
    rounds: usize,
    iters: usize,
    lambda: f32,
    log: impl Fn(&str),
) -> GlyphSet {
    let mut set = init;
    for s in samples.iter_mut() {
        fit_pens(s, &set, true);
    }
    log(&format!(
        "  placed {} samples: RMSE {:.1} grey levels over the ink",
        samples.len(),
        ink_rmse(samples, &set)
    ));
    for round in 0..rounds {
        fit_shapes(samples, &mut set, lambda, iters);
        for s in samples.iter_mut() {
            fit_pens(s, &set, false);
        }
        log(&format!(
            "  round {}: RMSE {:.1} grey levels over the ink",
            round + 1,
            ink_rmse(samples, &set)
        ));
    }
    set
}

/// RMSE (grey levels) over pixels within 1 px of observed ink.
pub fn ink_rmse(samples: &[Sample], set: &GlyphSet) -> f64 {
    ink_rmse_of(samples.iter(), set)
}

/// `ink_rmse` per field.
pub fn ink_rmse_by_field(samples: &[Sample], set: &GlyphSet) -> Vec<(Field, f64)> {
    let mut fields: Vec<Field> = samples.iter().map(|s| s.field).collect();
    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .map(|f| (f, ink_rmse_of(samples.iter().filter(|s| s.field == f), set)))
        .collect()
}

fn ink_rmse_of<'a>(samples: impl Iterator<Item = &'a Sample>, set: &GlyphSet) -> f64 {
    let (mut e, mut n) = (0f64, 0usize);
    for s in samples {
        let o = &s.obs;
        let r = render(&placed(set, &s.text, &s.pens), o.k, o.w, o.h);
        for (i, &rv) in r.iter().enumerate() {
            let (x, y) = (i % o.w, i / o.w);
            let near = (y.saturating_sub(1)..(y + 2).min(o.h)).any(|yy| {
                (x.saturating_sub(1)..(x + 2).min(o.w))
                    .any(|xx| o.img[yy * o.w + xx] >= 60.0 / 255.0)
            });
            if o.mask[i] && near {
                e += o.wt[i] as f64 * ((rv - o.img[i]) as f64).powi(2);
                n += 1;
            }
        }
    }
    (e / n.max(1) as f64).sqrt() * 255.0
}

/// Sets each character's advance to the median gap (atlas units) between its pen and the next
/// one's in the fitted samples; characters never followed by another get the median of all.
pub fn learn_advances(samples: &[Sample], set: &mut GlyphSet) {
    let mut by: HashMap<char, Vec<f32>> = HashMap::new();
    let mut all = vec![];
    for s in samples {
        for (c, w) in s.text.chars().zip(s.pens.windows(2)) {
            let gap = (w[1].0 - w[0].0) / s.obs.k;
            by.entry(c).or_default().push(gap);
            all.push(gap);
        }
    }
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    if all.is_empty() {
        return;
    }
    let fallback = median(&mut all);
    for g in set.glyphs.iter_mut() {
        g.adv = by.get_mut(&g.ch).map_or(fallback, median);
    }
}

/// Median pen height per field below its label box top, in atlas units.
pub fn pen_heights(samples: &[Sample]) -> HashMap<Field, f32> {
    let mut by: HashMap<Field, Vec<f32>> = HashMap::new();
    for s in samples {
        for p in &s.pens {
            by.entry(s.field)
                .or_default()
                .push((p.1 - s.obs.top) / s.obs.k);
        }
    }
    by.into_iter()
        .map(|(f, mut v)| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (f, v[v.len() / 2])
        })
        .collect()
}

/// Characters the samples show.
pub fn seen(samples: &[Sample]) -> String {
    CHARS
        .chars()
        .filter(|c| samples.iter().any(|s| s.text.contains(*c)))
        .collect()
}

/// Gives `set` the other label type's shape for every character it has not seen.
pub fn fill_unseen(
    set: &mut GlyphSet,
    seen: &str,
    other: &GlyphSet,
    other_seen: &str,
) -> Vec<char> {
    let mut taken = vec![];
    for g in set.glyphs.iter_mut() {
        if !seen.contains(g.ch) && other_seen.contains(g.ch) {
            if let Some(o) = other.get(g.ch) {
                *g = o.clone();
                taken.push(g.ch);
            }
        }
    }
    taken
}
