//! Reads one field: finds the string of glyphs, each at its own sub-pixel position, whose
//! rendering best explains the observed coverage, without splitting it into characters first.
//!
//! 1. Every character is pre-rendered at 4x4 sub-pixel pen phases (`Templates`).
//! 2. For each pen height near the calibrated one, a dynamic programme over x finds the best
//!    chains of glyphs, neighbours one advance apart (the advance learned with the shapes,
//!    +-0.75 px, since the game rounds pens to pixels); the K best per position are kept.
//! 3. The best distinct strings are re-scored jointly (overlapping edges counted once), each
//!    glyph's position refined, and invalid strings (leading zeros, misplaced slash) dropped.
//! 4. Confidence: each character is replaced by every other one in turn; its margin is how much
//!    worse (RMSE, grey levels) the best substitute fits. The string's margin is the gap to the
//!    runner-up string. Once one margin is below `MIN_MARGIN` the rest are not worked out.

use super::observe::Obs;
use super::shapes::{stamp, Glyph, GlyphSet};

/// Sub-pixel pen phases per pixel.
pub const PHASES: usize = 4;
/// Best chains kept per position and length.
const K: usize = 2;
/// Distinct strings re-scored jointly, and how many of the best of those get refined.
const TOP_STRINGS: usize = 30;
const REFINED: usize = 5;
/// Refinement passes over all glyphs at most.
const MAX_REFINE_PASSES: usize = 4;
/// Pen heights searched around the calibrated one, in phases (1 px either way).
const Y_RANGE: i32 = 4;
/// Pen advance between neighbours: the learned one +- this many pixels.
const ADV_JITTER: f32 = 0.75;

/// One character pre-rendered at one sub-pixel phase: coverage relative to the integer pen.
pub struct Tpl {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    v: Vec<f32>,
    /// v^2 per pixel, and its sum
    v2: Vec<f32>,
    e: f32,
}

/// All characters of a glyph set at one font size.
pub struct Templates {
    pub k: f32,
    chars: Vec<char>,
    /// Pen advance per character, pixels.
    adv: Vec<f32>,
    /// [char][phase_y * PHASES + phase_x]
    tpl: Vec<Vec<Tpl>>,
}

impl Templates {
    pub fn build(set: &GlyphSet, k: f32) -> Templates {
        let ph = PHASES;
        let mut chars = vec![];
        let mut adv = vec![];
        let mut tpl = vec![];
        for g in &set.glyphs {
            let (cw, chh) = (
                (g.gw as f32 * k).ceil() as usize + 4,
                (g.gh as f32 * k).ceil() as usize + 4,
            );
            let (cx, cy) = ((-g.ox * k).ceil() + 2.0, (-g.oy * k).ceil() + 2.0);
            let mut v = vec![];
            for py in 0..ph {
                for px in 0..ph {
                    let mut r = vec![0f32; cw * chh];
                    let pen = (cx + px as f32 / ph as f32, cy + py as f32 / ph as f32);
                    stamp(g, k, pen.0, pen.1, cw, chh, |p, c, a| r[p] += a * g.cov[c]);
                    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
                    for y in 0..chh {
                        for x in 0..cw {
                            if r[y * cw + x] > 1e-4 {
                                x0 = x0.min(x);
                                x1 = x1.max(x);
                                y0 = y0.min(y);
                                y1 = y1.max(y);
                            }
                        }
                    }
                    if x0 == usize::MAX {
                        (x0, x1, y0, y1) = (0, 0, 0, 0);
                    }
                    let (w, h) = (x1 + 1 - x0, y1 + 1 - y0);
                    let vv: Vec<f32> = (0..w * h)
                        .map(|i| r[(y0 + i / w) * cw + x0 + i % w].min(1.0))
                        .collect();
                    let v2: Vec<f32> = vv.iter().map(|x| x * x).collect();
                    let e = v2.iter().sum();
                    v.push(Tpl {
                        x0: x0 as i32 - cx as i32,
                        y0: y0 as i32 - cy as i32,
                        w,
                        h,
                        v: vv,
                        v2,
                        e,
                    });
                }
            }
            chars.push(g.ch);
            adv.push(g.adv * k);
            tpl.push(v);
        }
        Templates { k, chars, adv, tpl }
    }

    fn get(&self, ci: usize, xq: i32, yq: i32) -> (&Tpl, i32, i32) {
        let ph = PHASES as i32;
        let (xi, fx) = (xq.div_euclid(ph), xq.rem_euclid(ph) as usize);
        let (yi, fy) = (yq.div_euclid(ph), yq.rem_euclid(ph) as usize);
        (&self.tpl[ci][fy * PHASES + fx], xi, yi)
    }

    fn index(&self, ch: char) -> Option<usize> {
        self.chars.iter().position(|&c| c == ch)
    }
}

/// Which strings a field may show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grammar {
    /// A number without leading zeros.
    Number,
    /// Two numbers separated by '/'.
    Ratio,
}

impl Grammar {
    fn alphabet(self) -> &'static str {
        match self {
            Grammar::Number => "0123456789",
            Grammar::Ratio => "0123456789/",
        }
    }

    pub fn valid(self, s: &str) -> bool {
        let number = |t: &str| {
            !t.is_empty()
                && t.chars().all(|c| c.is_ascii_digit())
                && (t == "0" || !t.starts_with('0'))
        };
        match self {
            Grammar::Number => number(s),
            Grammar::Ratio => s
                .split_once('/')
                .is_some_and(|(a, b)| number(a) && number(b)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Reading {
    pub text: String,
    /// Weighted RMSE (grey levels, in units of the text's contrast) over the ink region.
    pub rmse: f64,
    /// Runner-up string's RMSE minus this one's (infinite if there is none).
    pub margin: f64,
    /// Per character: the best substitute's RMSE minus this one's (only up to the first one
    /// below `MIN_MARGIN`, or none if the string margin already is).
    pub char_margin: Vec<f64>,
    /// Glyph pens in region pixels.
    pub pens: Vec<(f32, f32)>,
    /// Time spent per stage, ms: pen height search, chain search, re-scoring, confidence.
    pub stage_ms: [f64; 4],
    /// Pen height in phases (the same for all glyphs before refinement).
    pub pen_yq: i32,
}

impl Reading {
    /// Smallest of the string margin and the character margins.
    pub fn confidence(&self) -> f64 {
        self.char_margin.iter().copied().fold(self.margin, f64::min)
    }
}

type Placed = (usize, i32, i32); // (char index, x phase, y phase)
/// A chain in the K-best search: (score, char, previous x, previous rank, previous state).
type Entry = (f32, u8, u32, u8, u8);

/// Exact cost of a joint reading: templates summed (capped at 1), weighted squared error on
/// scored pixels, any ink on unscored pixels or outside the region counted as wrong.
fn joint_cost(o: &Obs, t: &Templates, glyphs: &[Placed]) -> f64 {
    let mut r = vec![0f32; o.w * o.h];
    let mut lost = 0f64;
    for &(ci, xq, yq) in glyphs {
        let (tp, xi, yi) = t.get(ci, xq, yq);
        for j in 0..tp.h {
            for i in 0..tp.w {
                let (px, py) = (xi + tp.x0 + i as i32, yi + tp.y0 + j as i32);
                let tv = tp.v[j * tp.w + i];
                if px >= 0 && py >= 0 && (px as usize) < o.w && (py as usize) < o.h {
                    r[py as usize * o.w + px as usize] += tv;
                } else {
                    lost += (tv * tv) as f64;
                }
            }
        }
    }
    for v in r.iter_mut() {
        *v = v.min(1.0);
    }
    lost + render_error(o, &r)
}

/// Per-pixel error terms of a field: a pixel at summed coverage `v` costs
/// `pw * (min(v, 1) - pt)^2`, with (pw, pt) = (weight, observed coverage) on scored pixels and
/// (1, 0) elsewhere (ink there is all error).
struct ErrTerms {
    w: usize,
    h: usize,
    pw: Vec<f32>,
    pt: Vec<f32>,
}

impl ErrTerms {
    fn new(o: &Obs) -> Self {
        let n = o.w * o.h;
        ErrTerms {
            w: o.w,
            h: o.h,
            pw: (0..n)
                .map(|i| if o.mask[i] { o.wt[i] } else { 1.0 })
                .collect(),
            pt: (0..n)
                .map(|i| if o.mask[i] { o.img[i] } else { 0.0 })
                .collect(),
        }
    }

    /// One pixel's error at summed coverage `v` (capped at 1).
    #[inline]
    fn at(&self, i: usize, v: f32) -> f64 {
        let d = (v.min(1.0) - self.pt[i]) as f64;
        self.pw[i] as f64 * (d * d)
    }

    /// Ink of template `tp` at pixel (gx, gy) (its box origin) that falls outside the region.
    fn lost(&self, tp: &Tpl, gx: i32, gy: i32) -> f64 {
        if gx >= 0 && gy >= 0 && gx as usize + tp.w <= self.w && gy as usize + tp.h <= self.h {
            return 0.0;
        }
        let mut l = 0f64;
        for j in 0..tp.h {
            for i in 0..tp.w {
                let (px, py) = (gx + i as i32, gy + j as i32);
                if px < 0 || py < 0 || px as usize >= self.w || py as usize >= self.h {
                    l += (tp.v[j * tp.w + i] as f64).powi(2);
                }
            }
        }
        l
    }
}

/// A reading's rendering kept up to date, so that changing one glyph costs only the pixels it
/// covers. Its cost equals `joint_cost` of its glyphs.
/// A canvas with one glyph taken out over the pixels its alternatives cover (`Canvas::hole`).
struct Hole {
    /// Pixel box x0, y0, x1, y1 in the region, and its width.
    r: (usize, usize, usize, usize),
    rw: usize,
    /// Summed coverage without the glyph, and each pixel's error at it.
    s: Vec<f32>,
    e0: Vec<f32>,
    /// Cost without the glyph.
    base: f64,
}

#[derive(Clone)]
struct Canvas<'a> {
    e: &'a ErrTerms,
    t: &'a Templates,
    /// Summed (uncapped) glyph coverage per pixel.
    sum: Vec<f32>,
    gs: Vec<Placed>,
    cost: f64,
    /// Per glyph: no refinement move improved it, and nothing it could reach has changed since.
    clean: Vec<bool>,
}

/// Pixels by which a glyph's box can grow or shift in one refinement move of at most one pixel
/// (4 phases): the box moves by up to 1 px and a phase's template may be 1 px wider.
const MOVE_REACH: usize = 2;

fn overlaps(a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

impl<'a> Canvas<'a> {
    fn new(e: &'a ErrTerms, t: &'a Templates, gs: &[Placed]) -> Self {
        let mut sum = vec![0f32; e.w * e.h];
        let mut lost = 0f64;
        for &g in gs {
            lost += Self::stamp(e, t, g, &mut sum, 1.0);
        }
        let cost = lost + (0..sum.len()).map(|i| e.at(i, sum[i])).sum::<f64>();
        Canvas {
            e,
            t,
            sum,
            gs: gs.to_vec(),
            cost,
            clean: vec![false; gs.len()],
        }
    }

    /// Adds `sign` times glyph `g` to `sum`; returns its ink outside the region.
    fn stamp(e: &ErrTerms, t: &Templates, g: Placed, sum: &mut [f32], sign: f32) -> f64 {
        let (tp, xi, yi) = t.get(g.0, g.1, g.2);
        let mut lost = 0f64;
        for j in 0..tp.h {
            for i in 0..tp.w {
                let (px, py) = (xi + tp.x0 + i as i32, yi + tp.y0 + j as i32);
                let tv = tp.v[j * tp.w + i];
                if px >= 0 && py >= 0 && (px as usize) < e.w && (py as usize) < e.h {
                    sum[py as usize * e.w + px as usize] += sign * tv;
                } else {
                    lost += (tv * tv) as f64;
                }
            }
        }
        lost
    }

    /// Pixel box of glyph `g`, clipped to the region: x0, y0, x1, y1.
    fn bbox(&self, g: Placed) -> (usize, usize, usize, usize) {
        let (tp, xi, yi) = self.t.get(g.0, g.1, g.2);
        let clip = |v: i32, n: usize| v.clamp(0, n as i32) as usize;
        (
            clip(xi + tp.x0, self.e.w),
            clip(yi + tp.y0, self.e.h),
            clip(xi + tp.x0 + tp.w as i32, self.e.w),
            clip(yi + tp.y0 + tp.h as i32, self.e.h),
        )
    }

    /// Cost if glyph `i` were `new` instead.
    fn cost_if(&self, i: usize, new: Placed) -> f64 {
        let old = self.gs[i];
        let (a, b) = (self.bbox(old), self.bbox(new));
        let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
        let (ta, ax, ay) = self.t.get(old.0, old.1, old.2);
        let (tb, bx, by) = self.t.get(new.0, new.1, new.2);
        // template box origins in region pixels
        let (ax, ay) = (ax + ta.x0, ay + ta.y0);
        let (bx, by) = (bx + tb.x0, by + tb.y0);
        let row = |tp: &'a Tpl, gy: i32, y: usize| -> &'a [f32] {
            let j = y as i32 - gy;
            if j >= 0 && (j as usize) < tp.h {
                &tp.v[j as usize * tp.w..][..tp.w]
            } else {
                &[]
            }
        };
        let val = |r: &[f32], gx: i32, x: usize| -> f32 {
            let i = x as i32 - gx;
            if i >= 0 && (i as usize) < r.len() {
                r[i as usize]
            } else {
                0.0
            }
        };
        let mut d = 0f64;
        for y in y0..y1 {
            let (ra, rb) = (row(ta, ay, y), row(tb, by, y));
            if ra.is_empty() && rb.is_empty() {
                continue;
            }
            let off = y * self.e.w;
            for x in x0..x1 {
                let p = off + x;
                let before = self.sum[p];
                let after = before - val(ra, ax, x) + val(rb, bx, x);
                if after != before {
                    d += self.e.at(p, after) - self.e.at(p, before);
                }
            }
        }
        self.cost + d + self.e.lost(tb, bx, by) - self.e.lost(ta, ax, ay)
    }

    /// Glyph `i` taken out over the pixels any of `cands` (alternatives for it) covers, so that
    /// each of them is costed by adding it alone (`Hole::cost`).
    fn hole(&self, i: usize, cands: &[Placed]) -> Hole {
        let old = self.gs[i];
        let mut r = self.bbox(old);
        for &g in cands {
            let b = self.bbox(g);
            r = (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3));
        }
        let (rw, rh) = (r.2 - r.0, r.3 - r.1);
        let mut s = vec![0f32; rw * rh];
        for y in 0..rh {
            let o = (r.1 + y) * self.e.w + r.0;
            s[y * rw..][..rw].copy_from_slice(&self.sum[o..o + rw]);
        }
        let (ta, ax, ay) = self.t.get(old.0, old.1, old.2);
        let (ax, ay) = (ax + ta.x0, ay + ta.y0);
        let mut before = 0f64;
        for j in 0..ta.h {
            let py = ay + j as i32;
            if py < r.1 as i32 || py >= r.3 as i32 {
                continue;
            }
            for k in 0..ta.w {
                let px = ax + k as i32;
                if px < r.0 as i32 || px >= r.2 as i32 {
                    continue;
                }
                let li = (py as usize - r.1) * rw + (px as usize - r.0);
                let gi = py as usize * self.e.w + px as usize;
                let v = s[li];
                let after = v - ta.v[j * ta.w + k];
                before += self.e.at(gi, after) - self.e.at(gi, v);
                s[li] = after;
            }
        }
        let e0: Vec<f32> = (0..rw * rh)
            .map(|li| self.e.at((r.1 + li / rw) * self.e.w + r.0 + li % rw, s[li]) as f32)
            .collect();
        Hole {
            r,
            rw,
            s,
            e0,
            base: self.cost + before - self.e.lost(ta, ax, ay),
        }
    }

    /// Cost with the hole's glyph replaced by `g` (one of the candidates it was made for).
    fn hole_cost(&self, h: &Hole, g: Placed) -> f64 {
        let (tp, gx, gy) = self.t.get(g.0, g.1, g.2);
        let (gx, gy) = (gx + tp.x0, gy + tp.y0);
        let e = self.e;
        let mut d = 0f64;
        // the candidate's box clipped to the hole (which holds all of it that is in the region)
        let jx0 = (h.r.0 as i32 - gx).max(0) as usize;
        let jx1 = ((h.r.2 as i32 - gx).max(0) as usize).min(tp.w);
        for j in 0..tp.h {
            let py = gy + j as i32;
            if py < h.r.1 as i32 || py >= h.r.3 as i32 || jx0 >= jx1 {
                continue;
            }
            let ly = py as usize - h.r.1;
            let lrow = ly * h.rw;
            let grow = py as usize * e.w;
            let mut acc = 0f32;
            for k in jx0..jx1 {
                let lx = (gx + k as i32) as usize - h.r.0;
                let li = lrow + lx;
                let gi = grow + h.r.0 + lx;
                let v = (h.s[li] + tp.v[j * tp.w + k]).min(1.0) - e.pt[gi];
                acc += e.pw[gi] * v * v - h.e0[li];
            }
            d += acc as f64;
        }
        h.base + d + e.lost(tp, gx, gy)
    }

    /// Replaces glyph `i` with `new`.
    fn set(&mut self, i: usize, new: Placed) {
        self.cost = self.cost_if(i, new);
        let old = self.gs[i];
        // glyphs whose moves cover the changed pixels must be tried again
        let (a, b) = (self.bbox(old), self.bbox(new));
        let changed = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
        for j in 0..self.gs.len() {
            let (x0, y0, x1, y1) = self.bbox(self.gs[j]);
            let reach = (
                x0.saturating_sub(MOVE_REACH),
                y0.saturating_sub(MOVE_REACH),
                x1 + MOVE_REACH,
                y1 + MOVE_REACH,
            );
            if j == i || overlaps(reach, changed) {
                self.clean[j] = false;
            }
        }
        Self::stamp(self.e, self.t, old, &mut self.sum, -1.0);
        Self::stamp(self.e, self.t, new, &mut self.sum, 1.0);
        self.gs[i] = new;
    }

    /// Moves each glyph in turn by up to `dx`/`dy` phases (at most 4), keeping any improvement,
    /// until a pass changes nothing (at most `MAX_REFINE_PASSES`), so that readings compared with
    /// each other are all refined as far. A glyph that found no better position is tried again
    /// only once a change reaches the pixels its moves cover; until then it would find none.
    fn refine(&mut self, dx: i32, dy: i32) {
        for _ in 0..MAX_REFINE_PASSES {
            if !self.refine_pass(dx, dy) {
                break;
            }
        }
    }

    fn refine_pass(&mut self, dx: i32, dy: i32) -> bool {
        let mut moved = false;
        for i in 0..self.gs.len() {
            if self.clean[i] {
                continue;
            }
            let b = self.gs[i];
            let mut moves = Vec::with_capacity(((2 * dx + 1) * (2 * dy + 1)) as usize);
            for ddy in -dy..=dy {
                for ddx in -dx..=dx {
                    if ddx != 0 || ddy != 0 {
                        moves.push((b.0, b.1 + ddx, b.2 + ddy));
                    }
                }
            }
            let hole = self.hole(i, &moves);
            // the hole's own estimate of the current cost, so that rounding cannot make the
            // glyph move for nothing
            let here = self.hole_cost(&hole, b);
            let mut best = (here, b);
            for &g in &moves {
                let c = self.hole_cost(&hole, g);
                if c < best.0 {
                    best = (c, g);
                }
            }
            if best.1 != b {
                self.set(i, best.1);
                moved = true;
            } else {
                self.clean[i] = true;
            }
        }
        moved
    }
}

/// The pen height of a field: searched around the calibrated one (region pixels), or already
/// known for this match (phases).
#[derive(Debug, Clone, Copy)]
pub enum PenY {
    Search(f32),
    Fixed(i32),
}

/// Reads the field. None if it shows no ink or nothing valid is found.
pub fn read(
    o: &Obs,
    t: &Templates,
    grammar: Grammar,
    max_len: usize,
    pen: PenY,
) -> Option<Reading> {
    const P: i32 = 24; // padding of the score image so templates never need bounds checks
    let ph = PHASES as i32;
    let phf = PHASES as f32;
    let cis: Vec<usize> = grammar
        .alphabet()
        .chars()
        .filter_map(|c| t.index(c))
        .collect();
    let nd = cis.len();
    if nd == 0 || o.w == 0 || o.h == 0 {
        return None;
    }
    // A glyph's gain (how much it lowers the error) is <T, A> - <T^2, W>, with A = 2 w I and
    // W = w on scored pixels, A = 0 and W = 1 elsewhere (ink there is all error), so that faint
    // text over a low-contrast background (small w) is weighed the same way as in the exact cost.
    let (aw, ah) = (o.w + 2 * P as usize, o.h + 2 * P as usize);
    let mut a = vec![0f32; aw * ah];
    let mut wq = vec![1f32; aw * ah];
    for y in 0..o.h {
        for x in 0..o.w {
            let i = y * o.w + x;
            if o.mask[i] {
                let p = (y + P as usize) * aw + x + P as usize;
                a[p] = 2.0 * o.wt[i] * o.img[i];
                wq[p] = o.wt[i];
            }
        }
    }
    let xlo = -4 * ph;
    let nx = (o.w as i32 * ph - xlo) as usize;
    // Only pens whose glyph would overlap ink can be part of the best chain (a glyph over no
    // ink only adds error, and numbers have no gaps), so the search is limited to those.
    let ink_cols: Vec<usize> = (0..o.w)
        .filter(|&x| (0..o.h).any(|y| o.mask[y * o.w + x] && o.img[y * o.w + x] >= 60.0 / 255.0))
        .collect();
    let (Some(&ink0), Some(&ink1)) = (ink_cols.first(), ink_cols.last()) else {
        return None;
    };
    let reach_left = t
        .tpl
        .iter()
        .flatten()
        .map(|tp| tp.x0 + tp.w as i32)
        .max()
        .unwrap_or(0);
    let reach_right = t.tpl.iter().flatten().map(|tp| tp.x0).min().unwrap_or(0);
    let xi_of = |px: i32| ((px * ph - xlo).clamp(0, nx as i32)) as usize;
    let (xi0, xi1) = (
        xi_of(ink0 as i32 - reach_left),
        xi_of(ink1 as i32 - reach_right + 2),
    );
    // advance window (phases) after each character
    let win: Vec<(i32, i32)> = cis
        .iter()
        .map(|&ci| {
            (
                ((t.adv[ci] - ADV_JITTER) * phf).round() as i32,
                ((t.adv[ci] + ADV_JITTER) * phf).round() as i32,
            )
        })
        .collect();
    // Gains of every character at every pen phase x in xi0..xi1 at height yq. Per character and x
    // phase class the template is the same, so the correlation runs over whole rows of pen
    // positions (vectorised); each gain still sums its terms in the order `gain` does.
    let mut row_buf: Vec<f32> = vec![];
    let mut gains_at = |yq: i32| -> Vec<f32> {
        let mut g = vec![0f32; nx * nd];
        if xi1 <= xi0 {
            return g;
        }
        let (q0, q1) = (xlo + xi0 as i32, xlo + xi1 as i32); // pen phases q0..q1
        for (di, &ci) in cis.iter().enumerate() {
            for fx in 0..ph {
                // integer pens x with 4x + fx in q0..q1
                let xa = (q0 - fx + ph - 1).div_euclid(ph);
                let xb = (q1 - 1 - fx).div_euclid(ph);
                if xb < xa {
                    continue;
                }
                let (tp, _, y) = t.get(ci, xa * ph + fx, yq);
                let n = (xb - xa + 1) as usize;
                let by = y + tp.y0 + P;
                // pens whose template fits the padded image; others get `gain`'s -e
                let bx_a = xa + tp.x0 + P;
                let ka = (-bx_a).max(0) as usize;
                let kb = ((aw as i32 - tp.w as i32 - bx_a + 1).max(0) as usize).min(n);
                row_buf.clear();
                row_buf.resize(n, 0.0);
                if by < 0 || by as usize + tp.h > ah || ka >= kb {
                    row_buf.fill(-tp.e);
                } else {
                    row_buf[..ka].fill(-tp.e);
                    row_buf[kb..].fill(-tp.e);
                    let out = &mut row_buf[ka..kb];
                    for j in 0..tp.h {
                        let o = (by as usize + j) * aw + (bx_a as usize + ka);
                        for i in 0..tp.w {
                            let (tv, t2) = (tp.v[j * tp.w + i], tp.v2[j * tp.w + i]);
                            let (ra, rw) = (&a[o + i..][..out.len()], &wq[o + i..][..out.len()]);
                            for k in 0..out.len() {
                                out[k] += ra[k] * tv - rw[k] * t2;
                            }
                        }
                    }
                }
                for k in 0..n {
                    let xi = ((xa + k as i32) * ph + fx - xlo) as usize;
                    g[xi * nd + di] = row_buf[k];
                }
            }
        }
        g
    };
    let clock = std::time::Instant::now();
    let mut stage_ms = [0f64; 4];
    let mut lap = |s: usize| stage_ms[s] = clock.elapsed().as_secs_f64() * 1e3;
    let maxl = max_len.max(1);
    // 1) pen height: best chain of any length for each candidate height (max-plus DP)
    let (yq0, y_range) = match pen {
        PenY::Search(y) => ((y * phf).round() as i32, Y_RANGE),
        PenY::Fixed(yq) => (yq, 0),
    };
    let mut ybest = (f32::MIN, yq0, vec![]);
    for yq in yq0 - y_range..=yq0 + y_range {
        if y_range == 0 {
            ybest = (0.0, yq, gains_at(yq));
            break;
        }
        let g = gains_at(yq);
        // f[x][l][d]: best chain ending with character d at x, length l+1
        let mut f = vec![f32::MIN; nx * maxl * nd];
        let mut top = f32::MIN;
        for x in xi0..xi1 {
            for l in 0..maxl {
                // the best predecessor does not depend on this character
                let prev = if l == 0 {
                    0.0
                } else {
                    let mut p = f32::MIN;
                    for (pd, &(lo, hi)) in win.iter().enumerate() {
                        for adv in lo..=hi {
                            let px = x as i32 - adv;
                            if px >= 0 {
                                p = p.max(f[(px as usize * maxl + l - 1) * nd + pd]);
                            }
                        }
                    }
                    p
                };
                if prev == f32::MIN {
                    continue;
                }
                for d in 0..nd {
                    let v = prev + g[x * nd + d];
                    f[(x * maxl + l) * nd + d] = v;
                    top = top.max(v);
                }
            }
        }
        if top > ybest.0 {
            ybest = (top, yq, g);
        }
    }
    let (_, yq, g) = ybest;
    lap(0);
    // 2) K-best chains at that height, per grammar state (a ratio: before or after its slash, so
    // that chains with a slash survive the pruning). best[(x, l, state)]: entries
    // (score, char, prev x, prev rank, prev state)
    let slash = cis.iter().position(|&ci| t.chars[ci] == '/');
    let ns = if grammar == Grammar::Ratio { 2 } else { 1 };
    let idx = |x: usize, l: usize, s: usize| (x * maxl + l) * ns + s;
    let mut best: Vec<[Entry; K]> = vec![[(f32::MIN, 0, 0, 0, 0); K]; nx * maxl * ns];
    let mut nbest: Vec<u8> = vec![0; nx * maxl * ns];
    let mut cand: Vec<(f32, u32, u8)> = Vec::with_capacity(256);
    let mut preds = [[(0f32, 0u32, 0u8); K]; 2];
    let mut npreds = [0usize; 2];
    for x in xi0..xi1 {
        for l in 0..maxl {
            // best predecessors one advance (of the predecessor's character) back, per
            // predecessor state; they do not depend on this character or state
            for ps in 0..ns {
                cand.clear();
                if l > 0 {
                    for (pd, &(lo, hi)) in win.iter().enumerate() {
                        for adv in lo..=hi {
                            let p = x as i32 - adv;
                            if p < 0 {
                                continue;
                            }
                            let c = idx(p as usize, l - 1, ps);
                            for (r, e) in best[c][..nbest[c] as usize].iter().enumerate() {
                                if e.1 as usize == pd {
                                    cand.push((e.0, p as u32, r as u8));
                                }
                            }
                        }
                    }
                }
                cand.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                npreds[ps] = cand.len().min(K);
                preds[ps][..npreds[ps]].copy_from_slice(&cand[..npreds[ps]]);
            }
            for s in 0..ns {
                let mut c = [(f32::MIN, 0u8, 0u32, 0u8, 0u8); K];
                let mut nc = 0usize;
                for d in 0..nd {
                    let is_slash = Some(d) == slash;
                    // the state before this character
                    let ps = if is_slash {
                        if s == 0 || l == 0 {
                            continue;
                        }
                        0
                    } else {
                        s
                    };
                    let gd = g[x * nd + d];
                    let mut push = |e: Entry| {
                        if nc == K && e.0 <= c[K - 1].0 {
                            return;
                        }
                        let i = c[..nc].iter().position(|x| e.0 > x.0).unwrap_or(nc);
                        let end = (nc + 1).min(K);
                        for j in (i + 1..end).rev() {
                            c[j] = c[j - 1];
                        }
                        c[i] = e;
                        nc = end;
                    };
                    if l == 0 {
                        if s == 0 {
                            push((gd, d as u8, u32::MAX, 0, 0));
                        }
                        continue;
                    }
                    for &(sc, p, r) in &preds[ps][..npreds[ps]] {
                        push((sc + gd, d as u8, p, r, ps as u8));
                    }
                }
                best[idx(x, l, s)] = c;
                nbest[idx(x, l, s)] = nc as u8;
            }
        }
    }
    // complete strings end in the last state
    let mut all: Vec<(f32, usize, usize, usize)> = vec![];
    for x in 0..nx {
        for l in 0..maxl {
            let c = idx(x, l, ns - 1);
            for (r, e) in best[c][..nbest[c] as usize].iter().enumerate() {
                all.push((e.0, x, l, r));
            }
        }
    }
    all.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let trace = |x: usize, l: usize, r: usize| -> Vec<Placed> {
        let (mut x, mut l, mut r, mut s) = (x, l, r, ns - 1);
        let mut gs = vec![];
        loop {
            let e = best[idx(x, l, s)][r];
            gs.push((cis[e.1 as usize], xlo + x as i32, yq));
            if e.2 == u32::MAX {
                break;
            }
            x = e.2 as usize;
            r = e.3 as usize;
            s = e.4 as usize;
            l -= 1;
        }
        gs.reverse();
        gs
    };
    lap(1);
    // 3) joint re-scoring of the best distinct valid strings
    let text_of = |gs: &[Placed]| -> String { gs.iter().map(|g| t.chars[g.0]).collect() };
    // weighted RMS: the costs weigh each pixel by its text/background contrast, so they are
    // divided by the summed weights of the ink region (not its pixel count); otherwise faint
    // text would get small margins however clearly it reads
    let n_ink = (0..o.w * o.h)
        .filter(|&i| o.mask[i] && near_ink(o, i))
        .map(|i| o.wt[i] as f64)
        .sum::<f64>()
        .max(1e-3);
    let to_rmse = |c: f64| (c / n_ink).sqrt() * 255.0;
    let et = ErrTerms::new(o);
    // unrefined cost of each candidate first; only the best REFINED get the per-glyph refinement
    // (it moves costs by a little, so a string far behind before it stays behind)
    let mut scored: Vec<(String, f64, Vec<Placed>, Vec<bool>)> = vec![];
    for &(_, x, l, r) in &all {
        if scored.len() >= TOP_STRINGS {
            break;
        }
        let gs = trace(x, l, r);
        let text = text_of(&gs);
        if !grammar.valid(&text) || scored.iter().any(|s| s.0 == text) {
            continue;
        }
        let cost = Canvas::new(&et, t, &gs).cost;
        let clean = vec![false; gs.len()];
        scored.push((text, cost, gs, clean));
    }
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    for s in scored.iter_mut().take(REFINED) {
        let mut canvas = Canvas::new(&et, t, &s.2);
        canvas.refine(1, 2);
        (s.1, s.2, s.3) = (canvas.cost, canvas.gs, canvas.clean);
    }
    // an empty field: no glyph at all
    let empty_cost = joint_cost(o, t, &[]);
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let (text, cost, gs, clean) = scored.first()?.clone();
    let rmse = to_rmse(cost);
    let runner_up = scored
        .get(1)
        .map(|s| s.1)
        .unwrap_or(f64::INFINITY)
        .min(empty_cost);
    let margin = to_rmse(runner_up) - rmse;
    lap(2);
    // 4) per-character margins: every other character at each position (x +-2 phases)
    let alphabet: Vec<usize> = cis.clone();
    let mut base = Canvas::new(&et, t, &gs);
    base.clean = clean;
    let mut char_margin = vec![];
    // a doubtful field stays unread whatever its other margins are, so they are not worked out
    let doubtful = |m: f64| m < super::MIN_MARGIN;
    for i in 0..gs.len() {
        if doubtful(char_margin.iter().copied().fold(margin, f64::min)) {
            break;
        }
        // raw cost of every substitute (best x offset as placed), then refinement in order of
        // raw cost while a substitute could still set the margin
        let subs: Vec<usize> = alphabet
            .iter()
            .copied()
            .filter(|&d| d != gs[i].0)
            .filter(|&d| {
                let mut alt = gs.clone();
                alt[i].0 = d;
                grammar.valid(&text_of(&alt))
            })
            .collect();
        let here = gs[i];
        let cands: Vec<Placed> = subs
            .iter()
            .flat_map(|&d| (-2..=2).map(move |dx| (d, here.1 + dx, here.2)))
            .collect();
        let hole = base.hole(i, &cands);
        let mut alts: Vec<(f64, Placed)> = vec![];
        for per_char in cands.chunks(5) {
            let mut best: Option<(f64, Placed)> = None;
            for &g in per_char {
                let c = base.hole_cost(&hole, g);
                if best.is_none_or(|b| c < b.0) {
                    best = Some((c, g));
                }
            }
            alts.extend(best);
        }
        alts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut m = f64::INFINITY;
        for (c, g) in alts {
            if to_rmse(c) - rmse >= m + 3.0 {
                break;
            }
            let mut alt = base.clone();
            alt.set(i, g);
            alt.refine(1, 2);
            m = m.min(to_rmse(c.min(alt.cost)) - rmse);
        }
        char_margin.push(m);
    }
    lap(3);
    for s in (1..4).rev() {
        stage_ms[s] -= stage_ms[s - 1];
    }
    let pens = gs
        .iter()
        .map(|g| (g.1 as f32 / phf, g.2 as f32 / phf))
        .collect();
    Some(Reading {
        text,
        rmse,
        margin,
        char_margin,
        pens,
        stage_ms,
        pen_yq: yq,
    })
}

fn near_ink(o: &Obs, i: usize) -> bool {
    let (x, y) = (i % o.w, i / o.w);
    (y.saturating_sub(1)..(y + 2).min(o.h)).any(|yy| {
        (x.saturating_sub(1)..(x + 2).min(o.w)).any(|xx| o.img[yy * o.w + xx] >= 60.0 / 255.0)
    })
}

/// Exact rendering cost of `text` at the given pens (training and diagnostics).
pub fn cost_at(o: &Obs, glyphs: &[(&Glyph, f32, f32)]) -> f64 {
    let r = super::shapes::render(glyphs, o.k, o.w, o.h);
    let inside: f64 = r.iter().map(|v| *v as f64).sum();
    let total: f64 = glyphs
        .iter()
        .map(|(g, _, _)| super::shapes::ink(g, o.k) as f64)
        .sum();
    (total - inside).max(0.0) + render_error(o, &r)
}

/// Weighted squared error of a rendering on scored pixels plus all ink on unscored ones.
pub fn render_error(o: &Obs, r: &[f32]) -> f64 {
    r.iter()
        .zip(&o.mask)
        .zip(o.wt.iter().zip(&o.img))
        .map(|((&v, &m), (&w, &i))| {
            if m {
                w as f64 * ((v - i) as f64).powi(2)
            } else {
                (v as f64).powi(2)
            }
        })
        .sum()
}
