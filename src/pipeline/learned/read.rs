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
//!    runner-up string.

use super::observe::Obs;
use super::shapes::{stamp, Glyph, GlyphSet};

/// Sub-pixel pen phases per pixel.
pub const PHASES: usize = 4;
/// Best chains kept per position and length.
const K: usize = 2;
/// Distinct strings re-scored jointly.
const TOP_STRINGS: usize = 30;
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
    /// sum of v^2
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
                    let e = vv.iter().map(|x| x * x).sum();
                    v.push(Tpl {
                        x0: x0 as i32 - cx as i32,
                        y0: y0 as i32 - cy as i32,
                        w,
                        h,
                        v: vv,
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
    /// RMSE (grey levels) over the ink region.
    pub rmse: f64,
    /// Runner-up string's RMSE minus this one's (infinite if there is none).
    pub margin: f64,
    /// Per character: the best substitute's RMSE minus this one's.
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

/// Moves each glyph in turn by up to `dx`/`dy` phases, keeping any improvement.
fn refine(o: &Obs, t: &Templates, gs: &mut [Placed], dx: i32, dy: i32) -> f64 {
    let mut cur = joint_cost(o, t, gs);
    for i in 0..gs.len() {
        let b = gs[i];
        let mut best = b;
        for ddy in -dy..=dy {
            for ddx in -dx..=dx {
                if ddx == 0 && ddy == 0 {
                    continue;
                }
                gs[i] = (b.0, b.1 + ddx, b.2 + ddy);
                let c = joint_cost(o, t, gs);
                if c < cur {
                    cur = c;
                    best = gs[i];
                }
            }
        }
        gs[i] = best;
    }
    cur
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
    // A = 2 w I on scored pixels, 0 elsewhere: a glyph's gain is <T, A> - sum T^2
    let (aw, ah) = (o.w + 2 * P as usize, o.h + 2 * P as usize);
    let mut a = vec![0f32; aw * ah];
    for y in 0..o.h {
        for x in 0..o.w {
            let i = y * o.w + x;
            if o.mask[i] {
                a[(y + P as usize) * aw + x + P as usize] = 2.0 * o.wt[i] * o.img[i];
            }
        }
    }
    // integral image of A: a window without ink has gain exactly -sum T^2
    let mut ii = vec![0f32; (aw + 1) * (ah + 1)];
    for y in 0..ah {
        let mut rs = 0.0;
        for x in 0..aw {
            rs += a[y * aw + x];
            ii[(y + 1) * (aw + 1) + x + 1] = ii[y * (aw + 1) + x + 1] + rs;
        }
    }
    let gain = |tp: &Tpl, x: i32, y: i32| -> f32 {
        let (bx, by) = (x + tp.x0 + P, y + tp.y0 + P);
        if bx < 0 || by < 0 || bx as usize + tp.w > aw || by as usize + tp.h > ah {
            return -tp.e;
        }
        let (x0, y0, x1, y1) = (
            bx as usize,
            by as usize,
            bx as usize + tp.w,
            by as usize + tp.h,
        );
        if ii[y1 * (aw + 1) + x1] - ii[y0 * (aw + 1) + x1] - ii[y1 * (aw + 1) + x0]
            + ii[y0 * (aw + 1) + x0]
            < 1e-3
        {
            return -tp.e;
        }
        let mut g = 0.0;
        for j in 0..tp.h {
            let row = &a[(by as usize + j) * aw + bx as usize..][..tp.w];
            let tr = &tp.v[j * tp.w..][..tp.w];
            for i in 0..tp.w {
                g += row[i] * tr[i];
            }
        }
        g - tp.e
    };
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
    let gains_at = |yq: i32| -> Vec<f32> {
        let mut g = vec![0f32; nx * nd];
        for xi in xi0..xi1 {
            let xq = xlo + xi as i32;
            for (di, &ci) in cis.iter().enumerate() {
                let (tp, x, y) = t.get(ci, xq, yq);
                g[xi * nd + di] = gain(tp, x, y);
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
    let mut best: Vec<Vec<Entry>> = vec![vec![]; nx * maxl * ns];
    for x in xi0..xi1 {
        for l in 0..maxl {
            for s in 0..ns {
                let mut c: Vec<Entry> = Vec::with_capacity(K + 1);
                // best predecessors one advance (of the predecessor's character) back, per
                // predecessor state; they do not depend on this character
                let preds: Vec<Vec<(f32, u32, u8)>> = (0..ns)
                    .map(|ps| {
                        let mut pred: Vec<(f32, u32, u8)> = vec![];
                        if l > 0 {
                            for (pd, &(lo, hi)) in win.iter().enumerate() {
                                for adv in lo..=hi {
                                    let p = x as i32 - adv;
                                    if p < 0 {
                                        continue;
                                    }
                                    for (r, e) in
                                        best[idx(p as usize, l - 1, ps)].iter().enumerate()
                                    {
                                        if e.1 as usize == pd {
                                            pred.push((e.0, p as u32, r as u8));
                                        }
                                    }
                                }
                            }
                        }
                        pred.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                        pred.truncate(K);
                        pred
                    })
                    .collect();
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
                        if c.len() == K && e.0 <= c[K - 1].0 {
                            return;
                        }
                        let i = c.iter().position(|x| e.0 > x.0).unwrap_or(c.len());
                        c.insert(i, e);
                        c.truncate(K);
                    };
                    if l == 0 {
                        if s == 0 {
                            push((gd, d as u8, u32::MAX, 0, 0));
                        }
                        continue;
                    }
                    for &(sc, p, r) in &preds[ps] {
                        push((sc + gd, d as u8, p, r, ps as u8));
                    }
                }
                best[idx(x, l, s)] = c;
            }
        }
    }
    // complete strings end in the last state
    let mut all: Vec<(f32, usize, usize, usize)> = vec![];
    for x in 0..nx {
        for l in 0..maxl {
            for (r, e) in best[idx(x, l, ns - 1)].iter().enumerate() {
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
    let n_ink = (0..o.w * o.h)
        .filter(|&i| o.mask[i] && near_ink(o, i))
        .count()
        .max(1) as f64;
    let to_rmse = |c: f64| (c / n_ink).sqrt() * 255.0;
    let mut scored: Vec<(String, f64, Vec<Placed>)> = vec![];
    let mut tried = 0;
    for &(_, x, l, r) in &all {
        if tried >= TOP_STRINGS {
            break;
        }
        let mut gs = trace(x, l, r);
        let text = text_of(&gs);
        if !grammar.valid(&text) || scored.iter().any(|s| s.0 == text) {
            continue;
        }
        tried += 1;
        let c = refine(o, t, &mut gs, 1, 2);
        scored.push((text, c, gs));
    }
    // an empty field: no glyph at all
    let empty_cost = joint_cost(o, t, &[]);
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let (text, cost, gs) = scored.first()?.clone();
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
    let mut char_margin = vec![];
    for i in 0..gs.len() {
        let mut m = f64::INFINITY;
        for &d in &alphabet {
            if d == gs[i].0 {
                continue;
            }
            // best x offset as placed, then (if it could set the margin) refined once
            let mut best: Option<(f64, Vec<Placed>)> = None;
            for dx in -2..=2 {
                let mut alt = gs.clone();
                alt[i] = (d, gs[i].1 + dx, gs[i].2);
                if !grammar.valid(&text_of(&alt)) {
                    continue;
                }
                let c = joint_cost(o, t, &alt);
                if best.as_ref().is_none_or(|b| c < b.0) {
                    best = Some((c, alt));
                }
            }
            if let Some((mut best_c, mut alt)) = best {
                if to_rmse(best_c) - rmse < m + 3.0 {
                    best_c = best_c.min(refine(o, t, &mut alt, 1, 2));
                }
                m = m.min(to_rmse(best_c) - rmse);
            }
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
