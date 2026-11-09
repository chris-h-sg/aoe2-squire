//! Turns the pixels of one field into observed text coverage: per pixel how much of it is text
//! (0-1), whether it is scored at all, and how much it counts.

use super::layout::{Anchor, Backing, FieldLayout};
use image::RgbImage;

/// One field's search region, as coverage of the text colour.
#[derive(Debug, Clone)]
pub struct Obs {
    pub w: usize,
    pub h: usize,
    /// Observed coverage (may leave 0-1 slightly where the model does not fit).
    pub img: Vec<f32>,
    /// Pixels scored by the comparison; ink drawn elsewhere counts as fully wrong.
    pub mask: Vec<bool>,
    /// Weight of each scored pixel: the contrast between text and background, (0-1]^2.
    pub wt: Vec<f32>,
    /// Region origin in frame pixels.
    pub x0: u32,
    pub y0: u32,
    /// Label box top within the region, pixels.
    pub top: f32,
    /// Screen pixels per atlas unit of this field's font.
    pub k: f32,
}

/// Text colour found for a field drawn over the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColour {
    White,
    Yellow,
    Red,
}

impl TextColour {
    pub fn rgb(self) -> [f32; 3] {
        match self {
            TextColour::White => [255.0, 255.0, 255.0],
            // Housed or queue-blocked population (game 1 frames: pure yellow).
            TextColour::Yellow => [255.0, 255.0, 0.0],
            TextColour::Red => [255.0, 40.0, 40.0],
        }
    }
}

/// Pixels at most this bright count as the black outline of outlined text (the icon boxes behind
/// it are about 28, so only pixels near the outline are scored; some slack for video).
const DARK_MAX: f32 = 8.0;
/// Outlined text: a pixel is scored only if its colour lies this close (per channel) to the
/// text colour times its coverage, i.e. it is text, outline or black backing, not icon art.
const OUTLINED_COLOUR_TOL: f32 = 12.0;
/// Panel text: a pixel is ink if its text-colour channels are this much brighter than the 30th
/// percentile of its 9x9 neighbourhood.
const INK_RISE: f32 = 20.0;
/// Panel text: largest per-channel distance (0-255) of an ink pixel from the background blended
/// towards the text colour; farther, the pixel is something else and is ignored.
const PANEL_COLOUR_TOL: f32 = 20.0;

/// Crops the field's search region from `frame` (whole frame or the panel strip; `origin` is
/// `frame`'s origin in screen pixels) and measures the text coverage. `colour` overrides the
/// layout's text colour. `text_len` (training, where the text is known) narrows the region to
/// that many characters from the anchored edge, so that icon art beside the text stays out.
/// None if the region leaves the frame.
pub fn observe(
    frame: &RgbImage,
    origin: (u32, u32),
    fl: &FieldLayout,
    u: f32,
    colour: [f32; 3],
    text_len: Option<usize>,
) -> Option<Obs> {
    let mut crop = fl.crop;
    if let Some(n) = text_len {
        // a digit's advance is 26.5 atlas units of a 64-unit em; generous margin
        let width = n as f32 * 27.0 * fl.point_size / 64.0 + 6.0;
        let (bx0, bx1) = (crop[0] + 2.0, crop[2] - 2.0);
        (crop[0], crop[2]) = match fl.anchor {
            Anchor::Left => (crop[0], (bx0 + width).min(crop[2])),
            Anchor::Right => ((bx1 - width).max(crop[0]), crop[2]),
            Anchor::Centre => (
                ((bx0 + bx1 - width) / 2.0).max(crop[0]),
                ((bx0 + bx1 + width) / 2.0).min(crop[2]),
            ),
        };
    }
    let x0 = (crop[0] * u).floor().max(0.0) as u32;
    let y0 = (crop[1] * u).floor().max(0.0) as u32;
    let x1 = (crop[2] * u).ceil() as u32;
    let y1 = (crop[3] * u).ceil() as u32;
    if x0 < origin.0
        || y0 < origin.1
        || x1 > origin.0 + frame.width()
        || y1 > origin.1 + frame.height()
        || x1 <= x0
        || y1 <= y0
    {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let rgb: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let p = frame
                .get_pixel(
                    x0 - origin.0 + (i % w) as u32,
                    y0 - origin.1 + (i / w) as u32,
                )
                .0;
            [p[0] as f32, p[1] as f32, p[2] as f32]
        })
        .collect();
    let (img, mask, wt) = match fl.backing {
        Backing::Outlined => outlined(&rgb, w, h, colour, true),
        Backing::OutlinedOverColour => outlined(&rgb, w, h, colour, false),
        Backing::Panel => panel(&rgb, w, h, colour),
    };
    let k = fl.point_size * u / 64.0;
    Some(Obs {
        w,
        h,
        img,
        mask,
        wt,
        x0,
        y0,
        top: fl.top * u - y0 as f32,
        k,
    })
}

fn near(w: usize, h: usize, x: usize, y: usize, r: usize, f: impl Fn(usize) -> bool) -> bool {
    (y.saturating_sub(r)..(y + r + 1).min(h))
        .any(|yy| (x.saturating_sub(r)..(x + r + 1).min(w)).any(|xx| f(yy * w + xx)))
}

/// Outlined text over black: coverage is the projection on the text colour. Scored: pixels
/// whose colour is consistent with that (text, outline or backing) within 2 px of black
/// (outline or backing), minus dim pixels away from any ink (the box border). Over a saturated
/// colour (`near_black` false) the colour test alone separates text from background: there the
/// thin side of the outline can stay above `DARK_MAX`, which would leave a whole stem unscored.
fn outlined(
    rgb: &[[f32; 3]],
    w: usize,
    h: usize,
    c: [f32; 3],
    near_black: bool,
) -> (Vec<f32>, Vec<bool>, Vec<f32>) {
    let cc: f32 = c.iter().map(|v| v * v).sum();
    let img: Vec<f32> = rgb
        .iter()
        .map(|p| (p[0] * c[0] + p[1] * c[1] + p[2] * c[2]) / cc)
        .collect();
    let bright: Vec<f32> = rgb.iter().map(|p| p[0].max(p[1]).max(p[2])).collect();
    let consistent: Vec<bool> = rgb
        .iter()
        .zip(&img)
        .map(|(p, &v)| {
            let v = v.clamp(0.0, 1.0);
            (0..3).all(|ch| (p[ch] - v * c[ch]).abs() <= OUTLINED_COLOUR_TOL)
        })
        .collect();
    let dark = |i: usize| bright[i] <= DARK_MAX;
    let inky = |i: usize| img[i] >= 60.0 / 255.0;
    let mask = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let dim = bright[i] > 4.0 && bright[i] < 30.0 && !near(w, h, x, y, 2, inky);
            consistent[i] && (!near_black || near(w, h, x, y, 2, dark)) && !dim
        })
        .collect();
    (img, mask, vec![1.0; w * h])
}

/// Text over the panel: the background under the text is estimated by inpainting the ink
/// pixels from their surroundings, then coverage is the least-squares `c` in
/// `I = bg (1 - c) + colour c`, weighted by the text/background contrast.
fn panel(rgb: &[[f32; 3]], w: usize, h: usize, c: [f32; 3]) -> (Vec<f32>, Vec<bool>, Vec<f32>) {
    // Brightness in the text colour's channels: how much of the colour is at least present.
    let chans: Vec<usize> = (0..3).filter(|&ch| c[ch] >= 0.3 * 255.0).collect();
    let m: Vec<f32> = rgb
        .iter()
        .map(|p| {
            chans
                .iter()
                .map(|&ch| p[ch] * 255.0 / c[ch])
                .fold(f32::MAX, f32::min)
        })
        .collect();
    let r = 4usize;
    let ink = if chans.iter().all(|&ch| c[ch] == 255.0) {
        // m is then the integer channel value itself
        ink_by_histogram(&m, w, h, r)
    } else {
        ink_by_selection(&m, w, h, r)
    };
    let dil: Vec<bool> = (0..w * h)
        .map(|i| near(w, h, i % w, i / w, 1, |j| ink[j]))
        .collect();
    let bg = inpaint(rgb, &dil, w, h);
    let mut cov = vec![0f32; w * h];
    let mut wt = vec![0f32; w * h];
    let mut mask = vec![true; w * h];
    for i in 0..w * h {
        let (mut num, mut den) = (0f32, 0f32);
        for ch in 0..3 {
            let d = c[ch] - bg[i][ch];
            num += (rgb[i][ch] - bg[i][ch]) * d;
            den += d * d;
        }
        cov[i] = if dil[i] { num / den.max(1.0) } else { 0.0 };
        wt[i] = den / (3.0 * 255.0 * 255.0);
        // A pixel whose colour, in the text colour's own channels, is not the background blended
        // towards the text colour (an icon's yellow highlight next to white text, say) is not
        // text: unscored, so that any ink drawn there counts as wrong.
        if dil[i] {
            let v = cov[i].clamp(0.0, 1.0);
            if chans.iter().any(|&ch| {
                (rgb[i][ch] - (bg[i][ch] + v * (c[ch] - bg[i][ch]))).abs() > PANEL_COLOUR_TOL
            }) {
                mask[i] = false;
            }
        }
    }
    (cov, mask, wt)
}

/// Ink: pixels brighter than the 30th percentile of their (2r+1)^2 neighbourhood (clipped to the
/// region) by more than `INK_RISE`.
fn ink_by_selection(m: &[f32], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut ink = vec![false; w * h];
    let mut win = Vec::with_capacity((2 * r + 1) * (2 * r + 1));
    for y in 0..h {
        for x in 0..w {
            win.clear();
            for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                win.extend_from_slice(
                    &m[yy * w + x.saturating_sub(r)..yy * w + (x + r + 1).min(w)],
                );
            }
            let n = win.len() * 3 / 10;
            let (_, p30, _) = win.select_nth_unstable_by(n, |a, b| a.partial_cmp(b).unwrap());
            ink[y * w + x] = m[y * w + x] > *p30 + INK_RISE;
        }
    }
    ink
}

/// The same as `ink_by_selection` for integer values 0-255, with a histogram of the window slid
/// along each row: the n-th smallest value (n = 30% of the window) is below `m - INK_RISE`
/// exactly when more than n window values are at most `m - INK_RISE - 1`.
fn ink_by_histogram(m: &[f32], w: usize, h: usize, r: usize) -> Vec<bool> {
    let rise = INK_RISE as i32;
    let val: Vec<u8> = m.iter().map(|&v| v as u8).collect();
    let mut ink = vec![false; w * h];
    for y in 0..h {
        let (ya, yb) = (y.saturating_sub(r), (y + r + 1).min(h));
        let mut fine = [0u16; 256];
        let mut coarse = [0u16; 16];
        let col = |x: usize, add: bool, fine: &mut [u16; 256], coarse: &mut [u16; 16]| {
            for yy in ya..yb {
                let v = val[yy * w + x] as usize;
                if add {
                    fine[v] += 1;
                    coarse[v >> 4] += 1;
                } else {
                    fine[v] -= 1;
                    coarse[v >> 4] -= 1;
                }
            }
        };
        for x in 0..(r + 1).min(w) {
            col(x, true, &mut fine, &mut coarse);
        }
        for x in 0..w {
            let len = (yb - ya) * ((x + r + 1).min(w) - x.saturating_sub(r));
            let n = len * 3 / 10;
            let t = val[y * w + x] as i32 - rise - 1;
            if t >= 0 {
                let t = t as usize;
                let mut below: usize = coarse[..t >> 4].iter().map(|&c| c as usize).sum();
                below += fine[t & !15..=t].iter().map(|&c| c as usize).sum::<usize>();
                ink[y * w + x] = below > n;
            }
            if x >= r {
                col(x - r, false, &mut fine, &mut coarse);
            }
            if x + r + 1 < w {
                col(x + r + 1, true, &mut fine, &mut coarse);
            }
        }
    }
    ink
}

/// Fills the `hole` pixels with a harmonic interpolation of the others (per channel): first
/// linearly along each row between the nearest known pixels, then Jacobi iterations.
fn inpaint(rgb: &[[f32; 3]], hole: &[bool], w: usize, h: usize) -> Vec<[f32; 3]> {
    let mut bg = rgb.to_vec();
    for y in 0..h {
        let row = y * w;
        let mut x = 0;
        while x < w {
            if !hole[row + x] {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && hole[row + x] {
                x += 1;
            }
            let left = (start > 0).then(|| rgb[row + start - 1]);
            let right = (x < w).then(|| rgb[row + x]);
            for xi in start..x {
                bg[row + xi] = match (left, right) {
                    (Some(l), Some(r)) => {
                        let t = (xi + 1 - start) as f32 / (x - start + 1) as f32;
                        [0, 1, 2].map(|ch| l[ch] * (1.0 - t) + r[ch] * t)
                    }
                    (Some(v), None) | (None, Some(v)) => v,
                    (None, None) => [0.0; 3],
                };
            }
        }
    }
    // each hole's neighbours (left, right, up, down, as far as they exist)
    let holes: Vec<(usize, [usize; 4], usize)> = (0..w * h)
        .filter(|&i| hole[i])
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut nb = [0usize; 4];
            let mut n = 0;
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h {
                    nb[n] = yy as usize * w + xx as usize;
                    n += 1;
                }
            }
            (i, nb, n)
        })
        .collect();
    // Jacobi iterations; only holes change, so two buffers that agree elsewhere take turns
    let mut next = bg.clone();
    for _ in 0..60 {
        for &(i, nb, n) in &holes {
            let mut acc = [0f32; 3];
            for &j in &nb[..n] {
                for ch in 0..3 {
                    acc[ch] += bg[j][ch];
                }
            }
            next[i] = acc.map(|v| v / n as f32);
        }
        std::mem::swap(&mut bg, &mut next);
    }
    bg
}

/// Colour of the text drawn over the panel: the mean of the brightest pixels decides between
/// white, yellow (housed population) and red.
pub fn text_colour(frame: &RgbImage, origin: (u32, u32), fl: &FieldLayout, u: f32) -> TextColour {
    let x0 = ((fl.crop[0] * u).floor().max(0.0) as u32).saturating_sub(origin.0);
    let y0 = ((fl.crop[1] * u).floor().max(0.0) as u32).saturating_sub(origin.1);
    let x1 = (((fl.crop[2] * u).ceil() as u32).saturating_sub(origin.0)).min(frame.width());
    let y1 = (((fl.crop[3] * u).ceil() as u32).saturating_sub(origin.1)).min(frame.height());
    let mut px: Vec<[f32; 3]> = vec![];
    for y in y0..y1 {
        for x in x0..x1 {
            let p = frame.get_pixel(x, y).0;
            px.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
    }
    if px.is_empty() {
        return TextColour::White;
    }
    px.sort_by(|a, b| {
        (b[0] + b[1] + b[2])
            .partial_cmp(&(a[0] + a[1] + a[2]))
            .unwrap()
    });
    let n = (px.len() / 40).max(3).min(px.len());
    let mean = px[..n]
        .iter()
        .fold([0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]])
        .map(|v| v / n as f32);
    let [r, g, b] = mean;
    if g < 0.6 * r && b < 0.6 * r {
        TextColour::Red
    } else if b < 0.75 * r.min(g) {
        TextColour::Yellow
    } else {
        TextColour::White
    }
}
